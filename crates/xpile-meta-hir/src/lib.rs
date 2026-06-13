//! Canonical meta-HIR.
//!
//! Every frontend in xpile lowers its language-specific AST to this
//! shared, intentionally minimal IR. The federated approach (see
//! `docs/specifications/xpile-architecture-v1.md`) means we start
//! lossy and grow as hybrid-transpile cases demand.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub name: String,
    pub source_lang: SourceLang,
    pub items: Vec<Item>,
    pub ffi_boundaries: Vec<FfiBoundary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SourceLang {
    Python,
    C,
    Cpp,
    Cuda,
    Ruchy,
    /// Rust as a source language — keystone for bidirectional Rust↔Ruchy
    /// and the Rust→GPU paths (Rust→PTX/WGSL/SPIR-V).
    Rust,
    /// Lean 4 as a source language — its executable subset (def, partial
    /// def, inductive, structure, instance, ...) lowers to Rust via the
    /// code lane. Theorem statements parse via the proof lane separately.
    Lean,
    /// POSIX shell (bash / zsh / sh dialects + Makefile / Dockerfile) —
    /// the bashrs merger domain. PMAT-037 / XPILE-BASHRS-MERGER-001
    /// adds this variant as scaffold (`bashrs-frontend` produces it,
    /// `bashrs-backend` consumes it, all other backends return
    /// `Unsupported`). Per `sub/bashrs-merger.md` Layer B, meta-HIR
    /// will grow shell-specific `Stmt::Cmd` / `Stmt::Pipeline` etc.
    /// variants at v0.2.0 — at v0.1.0 a `SourceLang::Shell` `Module`
    /// is structurally empty (no items / no boundaries), validating
    /// only that the dispatch and SourceLang lane are wired.
    Shell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    Function(Function),
    /// A module-level constant — Python `NAME = <literal>`. PMAT-502bj
    /// (Tranche 2). First cut: `int` / `bool` / `float` values (which map
    /// to a Rust `const`); `str` constants need `&str` (deferred). Rust/
    /// Ruchy emit `const <name>: <ty> = <value>;`; Lean refuses. The
    /// frontend records the constant's type so references in function
    /// bodies type correctly.
    Const {
        name: String,
        ty: Type,
        value: Expr,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Type,
    /// Function body. A sequence of zero or more [`Stmt`] followed by a
    /// trailing return expression — the invariant that every function
    /// terminates by yielding exactly one value, kept explicit.
    pub body: Block,
}

impl Function {
    /// Returns the list of contract IDs that govern this function.
    /// Drives codegen citation emission (PMAT-011): each ID returned
    /// here will appear as `// xpile-contract: <ID>` (Rust/Ruchy) or
    /// `@[xpile_contract "<ID>"]` (Lean) next to the emitted function.
    ///
    /// Per v0.1.0's single Layer-1 contract, this returns
    /// `["C-PY-INT-ARITH"]` if and only if the function body uses any
    /// i64 arithmetic / bitwise / shift / power / unary-neg operator —
    /// the operations whose overflow / wrapping the contract bounds.
    /// Comparisons (`==`, `<`, etc.), logicals (`&&`, `!`), and
    /// constant-only / call-only bodies don't trigger the citation.
    pub fn applicable_contracts(&self) -> Vec<&'static str> {
        if self.uses_int_arithmetic() {
            vec!["C-PY-INT-ARITH"]
        } else {
            Vec::new()
        }
    }

    /// True if any expression in the function body uses an op that
    /// `C-PY-INT-ARITH` governs (overflow-prone arithmetic, bitwise,
    /// shifts, power, or unary negation). Walks all Stmt + Expr
    /// reachable from the body.
    pub fn uses_int_arithmetic(&self) -> bool {
        for stmt in &self.body.stmts {
            if stmt_has_int_arith(stmt) {
                return true;
            }
        }
        expr_has_int_arith(&self.body.trailing_return)
    }
}

fn stmt_has_int_arith(s: &Stmt) -> bool {
    match s {
        // PMAT-479 (R10): early return — recurse into the returned expr.
        Stmt::Return(e) => expr_has_int_arith(e),
        Stmt::Let { value, .. } | Stmt::Assign { value, .. } => expr_has_int_arith(value),
        // PMAT-504: a closure binding — recurse into the body expression.
        Stmt::ClosureLet { body, .. } => expr_has_int_arith(body),
        // PMAT-494b: tuple unpacking — recurse into the unpacked value.
        Stmt::LetTuple { value, .. } => expr_has_int_arith(value),
        Stmt::While { cond, body } => {
            if expr_has_int_arith(cond) {
                return true;
            }
            body.iter().any(stmt_has_int_arith)
        }
        // PMAT-478 (R9): if/else statement — recurse cond + both bodies.
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_has_int_arith(cond)
                || then_body.iter().any(stmt_has_int_arith)
                || else_body.iter().any(stmt_has_int_arith)
        }
        // PMAT-502bk: loop-control statements carry no expression.
        Stmt::Continue | Stmt::Break => false,
        // PMAT-502bw: `print(a, b, …)` — recurse into the argument exprs
        // (an arithmetic-using arg, e.g. `print(a + b)`, propagates).
        Stmt::Print { args, .. } => args.iter().any(expr_has_int_arith),
        // PMAT-458: for-each over a collection. The `iter` and `body`
        // are recursed; an arithmetic-using collection (e.g.,
        // `for x in [a+b, c*d]:`) propagates the citation requirement.
        Stmt::ForEach { iter, body, .. } => {
            if expr_has_int_arith(iter) {
                return true;
            }
            body.iter().any(stmt_has_int_arith)
        }
        // PMAT-495: paired for-loop — recurse into iter, the zip operand
        // (if any), and the body.
        Stmt::ForEachPair {
            iter, kind, body, ..
        } => {
            if expr_has_int_arith(iter) {
                return true;
            }
            if let PairIterKind::Zip(other) = kind {
                if expr_has_int_arith(other) {
                    return true;
                }
            }
            body.iter().any(stmt_has_int_arith)
        }
        // PMAT-460: list.append() — recurse into the elem expression.
        Stmt::ListAppend { elem, .. } => expr_has_int_arith(elem),
        // PMAT-500b: set.add() — recurse into the elem expression.
        Stmt::SetAdd { elem, .. } => expr_has_int_arith(elem),
        // PMAT-502av: set.remove()/discard() — recurse into the elem.
        Stmt::SetRemove { elem, .. } => expr_has_int_arith(elem),
        // PMAT-502ap: in-place list mutators carry no sub-expression.
        Stmt::ListMutate { .. } => false,
        // PMAT-502aq: list.extend() — recurse into the other-list expr.
        Stmt::ListExtend { other, .. } => expr_has_int_arith(other),
        // PMAT-502bb: dict.update() — recurse into the other-dict expr.
        Stmt::DictUpdate { other, .. } => expr_has_int_arith(other),
        // PMAT-502ar: list.insert() — recurse into index and elem.
        Stmt::ListInsert { index, elem, .. } => {
            expr_has_int_arith(index) || expr_has_int_arith(elem)
        }
        // PMAT-461: indexed assignment — recurse into both index and
        // value expressions (either may carry arithmetic).
        Stmt::IndexAssign { index, value, .. } => {
            expr_has_int_arith(index) || expr_has_int_arith(value)
        }
        // PMAT-466 (v0.2.0 Track 1.C): dict keyed assignment — recurse
        // into both key and value expressions.
        Stmt::DictSet { key, value, .. } => expr_has_int_arith(key) || expr_has_int_arith(value),
        // PMAT-502at: del coll[key] — recurse into the key expression.
        Stmt::DelItem { key, .. } => expr_has_int_arith(key),
        Stmt::Assert { cond, msg } => {
            expr_has_int_arith(cond) || msg.as_ref().is_some_and(expr_has_int_arith)
        }
        // PMAT-503a: raise — recurse into the panic message expression.
        Stmt::Raise { message } => expr_has_int_arith(message),
        // PMAT-039: shell commands are governed by `C-BASHRS-POSIX-IDEMPOTENCE`,
        // not `C-PY-INT-ARITH`. The args are `Vec<String>` (literal
        // tokens) — no arithmetic operands.
        Stmt::Cmd { .. } => false,
        // PMAT-041: pipelines compose Cmds — recurse into each stage
        // for completeness (currently every stage is a Cmd, so this is
        // always false in practice).
        Stmt::Pipeline { stages } => stages.iter().any(stmt_has_int_arith),
        // PMAT-051: shell variable assignment carries a value Expr;
        // recurse for completeness (currently the value is always
        // a shell-domain Expr, so this is always false in practice).
        Stmt::ShellAssign { value, .. } => expr_has_int_arith(value),
        // PMAT-048: shell loops compose statements; recurse into
        // body + (where applicable) cond / items.
        Stmt::ShellLoop { kind, body } => {
            let kind_has = match kind {
                LoopKind::For { items, .. } => items.iter().any(expr_has_int_arith),
                LoopKind::While { cond } | LoopKind::Until { cond } => expr_has_int_arith(cond),
            };
            kind_has || body.iter().any(stmt_has_int_arith)
        }
    }
}

fn expr_has_int_arith(e: &Expr) -> bool {
    match e {
        Expr::Ident(_) | Expr::LitInt(_) | Expr::LitBool(_) | Expr::Unit => false,
        // PMAT-477 (R8): float arithmetic is governed by float
        // semantics (IEEE-754 saturation), not C-PY-INT-ARITH's
        // integer-overflow analysis. Literal carries no operands;
        // FloatBinOp recurses defensively (float subtrees never carry
        // int arithmetic in practice).
        Expr::LitFloat(_) => false,
        Expr::FloatBinOp { lhs, rhs, .. } => expr_has_int_arith(lhs) || expr_has_int_arith(rhs),
        Expr::BinOp { op, lhs, rhs } => {
            if binop_is_int_arith(*op) {
                return true;
            }
            expr_has_int_arith(lhs) || expr_has_int_arith(rhs)
        }
        Expr::UnOp { op, operand } => {
            if matches!(op, UnOp::Neg) {
                return true;
            }
            expr_has_int_arith(operand)
        }
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_has_int_arith(cond)
                || expr_has_int_arith(then_expr)
                || expr_has_int_arith(else_expr)
        }
        Expr::Call { args, .. } => args.iter().any(expr_has_int_arith),
        // PMAT-042: string-literal Expr variants carry no arithmetic
        // operands; they're governed by C-BASHRS-POSIX-IDEMPOTENCE
        // (or, for Python `str` literals at v0.2.0, by
        // C-XLATE-PY-STR-TO-RUST-STRING), not C-PY-INT-ARITH.
        Expr::LitStr(_) | Expr::QuotedString { .. } => false,
        // PMAT-451: string concatenation is a str-domain operation;
        // operands type as `Type::Str` and never contribute to
        // C-PY-INT-ARITH's overflow analysis. Recurse defensively in
        // case future lowering ever nests an int-arith expression
        // inside a string-typed position (unlikely but cheap).
        Expr::Concat { lhs, rhs } => expr_has_int_arith(lhs) || expr_has_int_arith(rhs),
        // PMAT-502bg: list concatenation — recurse into both operands.
        Expr::ListConcat { lhs, rhs } => expr_has_int_arith(lhs) || expr_has_int_arith(rhs),
        // PMAT-502bh: str.format — recurse into each formatted arg.
        Expr::StrFormat { args, .. } => args.iter().any(expr_has_int_arith),
        // PMAT-502cd: `s[i]` over a string — recurse into both operands
        // (the index may be an arithmetic expression).
        Expr::StrCharAt { string, index } => {
            expr_has_int_arith(string) || expr_has_int_arith(index)
        }
        // PMAT-502cl: string-chars — recurse into the string expr.
        Expr::StrChars { string } => expr_has_int_arith(string),
        // PMAT-502cm: ord/chr — recurse into the value expr.
        Expr::Ord { value } | Expr::Chr { value } => expr_has_int_arith(value),
        // PMAT-502am: formatted f-string field — recurse into the value.
        Expr::FormatSpec { value, .. } => expr_has_int_arith(value),
        // PMAT-492: string methods are str-domain; recurse into the
        // receiver and any args defensively (mirrors Concat).
        Expr::StrMethod { recv, args, .. } => {
            expr_has_int_arith(recv) || args.iter().any(expr_has_int_arith)
        }
        // PMAT-494: tuple literal — recurse into each element (a tuple of
        // computed ints can carry overflow-prone arithmetic).
        Expr::TupleLit(elems) => elems.iter().any(expr_has_int_arith),
        // PMAT-502q: tuple constant-index — recurse into the tuple expr.
        Expr::TupleIndex { tuple, .. } => expr_has_int_arith(tuple),
        // PMAT-496: slice — recurse into collection + bound expressions.
        // PMAT-502r: bounds are optional (open-ended slices).
        Expr::Slice {
            collection, lo, hi, ..
        } => {
            expr_has_int_arith(collection)
                || lo.as_deref().is_some_and(expr_has_int_arith)
                || hi.as_deref().is_some_and(expr_has_int_arith)
        }
        // PMAT-498: numeric builtin — recurse into each arg.
        Expr::NumBuiltin { args, .. } => args.iter().any(expr_has_int_arith),
        // PMAT-498b: sum — recurse into the list expression.
        Expr::Sum { list, .. } => expr_has_int_arith(list),
        // PMAT-502j: all(xs)/any(xs) — recurse into the bool list.
        Expr::BoolReduce { list, .. } => expr_has_int_arith(list),
        // PMAT-502m: int(x)/float(x) — recurse into the converted value.
        Expr::NumCast { value, .. } => expr_has_int_arith(value),
        // PMAT-502ad: str(x) — recurse into the converted value.
        Expr::ToStr { value, .. } => expr_has_int_arith(value),
        // PMAT-502ak: round(x) — recurse into the rounded value.
        Expr::RoundToInt { value } => expr_has_int_arith(value),
        // PMAT-502al: round(x, n) — recurse into the value and ndigits.
        Expr::RoundToDigits { value, ndigits } => {
            expr_has_int_arith(value) || expr_has_int_arith(ndigits)
        }
        // PMAT-502k: seq * n — recurse into both the sequence and count.
        Expr::Repeat { seq, n } => expr_has_int_arith(seq) || expr_has_int_arith(n),
        // PMAT-502c: sorted — recurse into the list expression.
        Expr::Sorted { list, key, .. } => {
            expr_has_int_arith(list) || key.as_ref().is_some_and(|k| expr_has_int_arith(&k.body))
        }
        Expr::Reversed { list } => expr_has_int_arith(list),
        // PMAT-502cj: list(range(...)) — recurse into the bound exprs.
        Expr::RangeList { start, stop, .. } => {
            expr_has_int_arith(start) || expr_has_int_arith(stop)
        }
        // PMAT-502ab: filter — recurse into the list and predicate body.
        Expr::Filter { list, lambda } => {
            expr_has_int_arith(list) || expr_has_int_arith(&lambda.body)
        }
        // PMAT-502ac: map — recurse into the list and transform body.
        Expr::Map { list, lambda } => expr_has_int_arith(list) || expr_has_int_arith(&lambda.body),
        // PMAT-502ai: enumerate/zip — recurse into the source list(s).
        Expr::Enumerate { list } => expr_has_int_arith(list),
        Expr::Zip { left, right } => expr_has_int_arith(left) || expr_has_int_arith(right),
        Expr::ListMinMax { list, key, .. } => {
            expr_has_int_arith(list) || key.as_ref().is_some_and(|k| expr_has_int_arith(&k.body))
        }
        // PMAT-502u: list query — recurse into the list and the arg.
        Expr::ListQuery { list, arg, .. } => expr_has_int_arith(list) || expr_has_int_arith(arg),
        // PMAT-502as: list.pop() — recurse into the list and optional index.
        Expr::ListPop { list, index } => {
            expr_has_int_arith(list) || index.as_ref().is_some_and(|i| expr_has_int_arith(i))
        }
        // PMAT-502au: dict.pop() — recurse into dict, key, and optional default.
        Expr::DictPop { dict, key, default } => {
            expr_has_int_arith(dict)
                || expr_has_int_arith(key)
                || default.as_ref().is_some_and(|d| expr_has_int_arith(d))
        }
        // PMAT-502ax: dict.setdefault() — recurse into dict, key, default.
        Expr::DictSetDefault { dict, key, default } => {
            expr_has_int_arith(dict) || expr_has_int_arith(key) || expr_has_int_arith(default)
        }
        // PMAT-500: set literal / membership — recurse defensively.
        Expr::SetLit(elems) => elems.iter().any(expr_has_int_arith),
        Expr::SetContains { set, elem } => expr_has_int_arith(set) || expr_has_int_arith(elem),
        // PMAT-502an: list membership — recurse into both sides.
        Expr::ListContains { list, elem } => expr_has_int_arith(list) || expr_has_int_arith(elem),
        // PMAT-502o: str substring containment — recurse into both sides.
        Expr::StrContains { haystack, needle } => {
            expr_has_int_arith(haystack) || expr_has_int_arith(needle)
        }
        // PMAT-502g: set algebra — recurse into both operands.
        Expr::SetOp { lhs, rhs, .. } => expr_has_int_arith(lhs) || expr_has_int_arith(rhs),
        // PMAT-455 (v0.2.0 Track 1.B): list literal — recurse into
        // each element. An int-typed element (`[1, 2, 3]`) doesn't
        // by itself involve overflow-prone arithmetic, but a list of
        // computed values (`[a + b, c * d]`) does.
        Expr::ListLit(elems) => elems.iter().any(expr_has_int_arith),
        // PMAT-462 (v0.2.0 Track 1.C): dict literal — recurse into
        // each key + value expression.
        Expr::DictLit(pairs) => pairs
            .iter()
            .any(|(k, v)| expr_has_int_arith(k) || expr_has_int_arith(v)),
        // PMAT-457 (v0.2.0 Track 1.B): list indexed access. Neither
        // collection nor index expressions are themselves arithmetic;
        // recurse defensively for computed indices like `xs[a + b]`.
        Expr::Index { collection, index } => {
            expr_has_int_arith(collection) || expr_has_int_arith(index)
        }
        // PMAT-466 (v0.2.0 Track 1.C): dict ops. The lookup/membership
        // operations are not themselves arithmetic; recurse into the
        // sub-expressions (a computed key or default may carry it).
        Expr::DictGet { dict, key } => expr_has_int_arith(dict) || expr_has_int_arith(key),
        Expr::DictGetOr { dict, key, default } => {
            expr_has_int_arith(dict) || expr_has_int_arith(key) || expr_has_int_arith(default)
        }
        Expr::DictContains { dict, key } => expr_has_int_arith(dict) || expr_has_int_arith(key),
        // PMAT-502v: dict view — recurse into the dict expression.
        Expr::DictView { dict, .. } => expr_has_int_arith(dict),
        // PMAT-459 (v0.2.0 Track 1.B): len() of a collection is not
        // itself arithmetic; recurse defensively into the inner expr.
        Expr::Len(inner) => expr_has_int_arith(inner),
        // PMAT-045: shell-variable references same disposition.
        Expr::ShellVar(_) => false,
        // PMAT-055: shell special parameters same disposition.
        Expr::ShellSpecial(_) => false,
        // PMAT-047: command substitution composes a Stmt; recurse
        // into the inner Stmt for completeness (currently every
        // such Stmt is a shell-domain Cmd, so this is always
        // false in practice).
        Expr::CommandSubstitution(inner) => stmt_has_int_arith(inner),
    }
}

fn binop_is_int_arith(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::FloorDiv
            | BinOp::Mod
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::Pow
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Zero or more `let` bindings (and, in the future, control-flow
    /// statements) executed before the trailing return.
    pub stmts: Vec<Stmt>,
    /// The expression whose value is the function's return value.
    pub trailing_return: Expr,
}

// PMAT-047: PartialEq needed so `Expr::CommandSubstitution(Box<Stmt>)`
// participates in Expr's existing `PartialEq` derive. Every field
// of every Stmt variant is itself PartialEq (String, Type, Expr,
// Vec<Stmt>), so the derive is mechanical.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    /// `return <expr>;` as an **early** return — a mid-body return,
    /// typically inside an [`Stmt::If`] branch (a guard clause:
    /// `if (n <= 1) { return 1; } return n * …;`). PMAT-479 (R10).
    ///
    /// The function's *final* value still flows through
    /// [`Block::trailing_return`]; this variant is only for the
    /// non-final returns. Rust/Ruchy emit `return <expr>;` (the trailing
    /// expression remains the fallthrough value). Lean refuses (early
    /// return needs a match/monadic encoding; Lean keeps the
    /// single-trailing-return shape at v0.2.0).
    Return(Expr),
    /// `if cond { then_body } else { else_body }` — an if/else as a
    /// *statement* (not the if-as-let-expression form). PMAT-478 (R9).
    /// Produced by the decy C frontend for `if (c) { … } else { … }`
    /// (C has no if-expression); the Python frontend keeps its
    /// if-as-let lowering for the assignment shape. `else_body` is empty
    /// for an `if` with no `else`.
    ///
    /// Backends: Rust/Ruchy emit `if cond { … } else { … }`; Lean
    /// refuses (the executable-subset encoding routes branches through
    /// the if-expression form, not statement-if) at v0.2.0.
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    /// `continue;` — Python `continue`. PMAT-502bk (Tranche 2). Skips to
    /// the next loop iteration. Rust/Ruchy emit `continue;`. The frontend
    /// rejects it inside a `range(...)` for-loop (which desugars to a
    /// `while` with a tail counter-increment that `continue` would skip);
    /// it is allowed inside list/dict for-loops (real Rust `for`) and
    /// `while` loops. Lean refuses (no loop-control encoding).
    Continue,
    /// `break;` — Python `break`. PMAT-502bk (Tranche 2). Exits the
    /// nearest loop. Rust/Ruchy emit `break;` (always safe). Lean refuses.
    Break,
    /// `print(a, b, …, sep=…, end=…)` — Python's `print` builtin.
    /// PMAT-502bw (Tranche 2); PMAT-502by added `sep`/`end`. Rust/Ruchy
    /// build a format string joining the args with `sep` and either use
    /// `println!` (when `end == "\n"`, the Python default) or `print!` with
    /// `end` appended (any other terminator, e.g. `end=""`). An empty `args`
    /// (bare `print()`) emits `println!();` (or `print!("…end…")`).
    /// PMAT-502bx: the frontend admits `I64`/`Str` (incl. f-strings →
    /// `String`) directly, wraps `F64` via `str(float)` and `Bool` via the
    /// `str(bool)` desugar, so Python's `2.0`/`True` formatting is matched;
    /// `sep`/`end` must be string literals (non-literal + `file=` deferred).
    /// Lean refuses (pure `def`s have no `IO`).
    Print {
        args: Vec<Expr>,
        sep: String,
        end: String,
    },
    /// `let [mut] name: ty = value;` — first binding of `name` in this
    /// scope. `mutable` is set by the frontend when the same name is
    /// reassigned later in the function (including inside a [`While`]
    /// loop body). Rust/Ruchy emission honors it (`let mut` vs `let`)
    /// to keep `clippy -D unused_mut` happy. PMAT-006.
    Let {
        name: String,
        ty: Type,
        value: Expr,
        #[serde(default)]
        mutable: bool,
    },
    /// `name = value;` — reassignment of a name previously introduced
    /// by [`Stmt::Let`]. PMAT-006.
    Assign { name: String, value: Expr },
    /// `let name = |p0: t0, p1: t1, …| { body };` — a first-class closure
    /// bound to a local. PMAT-504 (Tranche 2). The Python source is
    /// `name = lambda p0, p1, …: <body>`. Each parameter carries its
    /// inferred type (`params: Vec<(name, type)>`, possibly empty for a
    /// nullary `lambda: <body>`); the return type is left to Rust's
    /// inference (no `-> R` annotation). The closure is then callable as
    /// `name(args…)` via the existing [`Expr::Call`] machinery (the
    /// frontend records the return type so the call site types
    /// correctly). Rust/Ruchy emit `let <name> = |<params>| { <body> };`;
    /// Lean refuses (first-class functions are a v0.3.0 sub-track).
    ClosureLet {
        name: String,
        params: Vec<(String, Type)>,
        body: Expr,
    },
    /// Tuple-destructuring binding — Python `a, b = <expr>`. PMAT-494b
    /// (sprint). `value` types as [`Type::Tuple`] with arity matching
    /// `names`. Rust/Ruchy emit `let (a, b, ...) = <value>;` (immutable
    /// first cut); Lean refuses. Nested / starred / subscript patterns
    /// are not supported at first cut (all targets must be plain names).
    LetTuple { names: Vec<String>, value: Expr },
    /// `while cond { body }` — Python `while cond: body`. The body is
    /// a list of statements (no trailing return; the loop body is not
    /// an expression). PMAT-006.
    While { cond: Expr, body: Vec<Stmt> },
    /// `list[index] = value` — Python `xs[i] = v` indexed assignment.
    /// PMAT-461, v0.2.0 Track 1.B. Companion of [`Stmt::ListAppend`]
    /// on the indexed-mutation side.
    ///
    /// Constraints (same as `ListAppend`):
    ///   - `list_name` must be a bound name typing as `Type::List(_)`.
    ///   - The receiver gets marked mutable so the emitter wraps it
    ///     in `mut name: Vec<T>`.
    ///   - Index must type as `Type::I64`; negative indices coerce
    ///     via `as usize` (underflow → panic) matching `Expr::Index`.
    ///
    /// Backends:
    ///   * Rust / Ruchy: `<list_name>[<index> as usize] = <value>;`.
    ///   * Lean: refuses at v0.2.0 first cut — same monadic-encoding
    ///     gap as `ListAppend` and `ForEach`.
    ///   * Shell: refuses.
    ///
    /// Governing contract: `C-XLATE-PY-LIST-TO-VEC` —
    /// iteration_order_preserved Bronze theorem implies in-place
    /// assignment preserves all other indices.
    IndexAssign {
        list_name: String,
        index: Expr,
        value: Expr,
    },
    /// `dict[key] = value` — Python `d[k] = v` keyed assignment.
    /// PMAT-466, v0.2.0 Track 1.C operations. Companion of
    /// [`Stmt::IndexAssign`] on the dict side; the frontend chooses
    /// between them by the receiver's inferred type.
    ///
    /// Constraints:
    ///   - `dict_name` must be a bound name typing as `Type::Dict(_, _)`.
    ///   - The receiver is marked mutable so the emitter wraps it in
    ///     `mut name: HashMap<K, V>` (the mutability pre-pass also
    ///     recognises subscript-target assigns, so a `let`-bound dict
    ///     mutated only via `d[k] = v` is correctly emitted `mut`).
    ///   - Insertion semantics: present key overwrites, absent key
    ///     inserts — exactly `HashMap::insert` and Python `d[k] = v`.
    ///
    /// Backends:
    ///   * Rust / Ruchy: `<dict_name>.insert(<key>, <value>);`.
    ///   * Lean: refuses at v0.2.0 first cut (no in-place mutation;
    ///     same monadic-encoding gap as `ListAppend`/`IndexAssign`).
    ///   * Shell: refuses.
    DictSet {
        dict_name: String,
        key: Expr,
        value: Expr,
    },
    /// `del coll[key]` — Python item deletion over a list or dict.
    /// PMAT-502at (Tranche 2). The frontend resolves `is_dict` from the
    /// receiver's inferred type and marks it mutable. For a list,
    /// removes the element at the (int) index, shifting the tail left;
    /// for a dict, removes the entry for the key. Both backends discard
    /// the removed value (Python `del` is a statement). Out-of-range /
    /// absent-key behaviour follows the underlying Rust method (a list
    /// index past the end panics, matching Python `IndexError`; a dict
    /// `del` of an absent key is a silent no-op here, whereas Python
    /// raises `KeyError` — a deferred fidelity gap).
    ///
    /// Backends:
    ///   * Rust / Ruchy: list → `<name>.remove((<key>) as usize);`;
    ///     dict → `<name>.remove(&(<key>));`.
    ///   * Lean: refuses (in-place mutation, same gap as `ListAppend`).
    DelItem {
        name: String,
        key: Expr,
        is_dict: bool,
    },
    /// `list.append(elem)` — Python `xs.append(v)` mutation. PMAT-460,
    /// v0.2.0 Track 1.B.
    ///
    /// Distinct from a generic `Expr::Call` because:
    ///   - Method-call mutation requires the receiver to be a name
    ///     (no chained `f().append(...)` shapes at v0.2.0).
    ///   - The receiver must be declared mutable (param or let-mut).
    ///   - The receiver must type as `Type::List(_)`.
    ///
    /// Backends:
    ///   * Rust / Ruchy: emit `<list_name>.push(<elem>);`. The frontend
    ///     ensures the receiver is `mut` so the call type-checks.
    ///   * Lean: refuses at v0.2.0 first cut — Lean has no in-place
    ///     mutation; the encoding would need a state-monad rewrite.
    ///     Deferred alongside other Lean v0.3.0 work.
    ///
    /// Governing contract: `C-XLATE-PY-LIST-TO-VEC` —
    /// `alias_observation_inserts_clone` Bronze theorem covers the
    /// alias-mediated-mutation semantics in Python (Rust's owned-Vec
    /// emission preserves the same observation by virtue of move
    /// semantics; aliased mutation through `&mut` is a v0.3.0+
    /// sub-track).
    ListAppend { list_name: String, elem: Expr },
    /// Set insertion — Python `s.add(x)`. PMAT-500b (Tranche 2). Mirrors
    /// [`Stmt::ListAppend`]; Rust/Ruchy emit `<set>.insert(<elem>);` (the
    /// receiver is marked mutable). Lean refuses.
    SetAdd { set_name: String, elem: Expr },
    /// Set element removal — Python `s.remove(x)` / `s.discard(x)`.
    /// PMAT-502av (Tranche 2). Both remove `elem` from the receiver
    /// (marked mutable). They differ only on an absent element:
    /// `error_if_absent` (Python `remove`) panics, matching `KeyError`;
    /// `discard` is a silent no-op. Rust `HashSet::remove` returns a
    /// `bool` (was-present), so:
    ///   * Rust / Ruchy: `remove` → `assert!(<set>.remove(&(<elem>)),
    ///     "xpile: KeyError: …");`; `discard` → `<set>.remove(&(<elem>));`.
    ///   * Lean: refuses (in-place mutation, same gap as `SetAdd`).
    SetRemove {
        set_name: String,
        elem: Expr,
        error_if_absent: bool,
    },
    /// In-place, zero-argument list mutation — Python `xs.sort()` /
    /// `xs.reverse()` / `xs.clear()`. PMAT-502ap (Tranche 2). These are
    /// the no-arg, in-place, `None`-returning list methods, lowered to
    /// the matching `Vec` method as an expression statement (the receiver
    /// is marked mutable). `of_float` is only consulted for [`ListMutateOp::Sort`]:
    /// `Vec<i64>` sorts via `.sort()`, but `Vec<f64>` has no `Ord` so it
    /// sorts via `.sort_by(|a, b| a.partial_cmp(b).unwrap())` (NaN panics,
    /// matching Python's undefined NaN-sort behaviour). Lean refuses
    /// (in-place mutation, same gap as `ListAppend`).
    ///
    /// Backends:
    ///   * Rust / Ruchy: `<list>.sort();` / `<list>.sort_by(…);` /
    ///     `<list>.reverse();` / `<list>.clear();`
    ///   * Lean: refuses.
    ListMutate {
        list_name: String,
        op: ListMutateOp,
        of_float: bool,
    },
    /// In-place list concatenation — Python `xs.extend(ys)`. PMAT-502aq
    /// (Tranche 2). Appends every element of `other` (any list-typed
    /// expression) to the receiver, which is marked mutable. Rust/Ruchy
    /// emit `<list>.extend((<other>).iter().cloned());` — cloning each
    /// element keeps `other` usable afterwards (matching Python, where
    /// `extend` does not consume its argument) and only needs `T: Clone`
    /// (true for every v0.2.0 element type). Lean refuses (in-place
    /// mutation, same gap as `ListAppend`).
    ListExtend { list_name: String, other: Expr },
    /// In-place dict merge — Python `d.update(other)`. PMAT-502bb
    /// (Tranche 2). Inserts every entry of `other` (a dict-typed
    /// expression) into the receiver, overwriting existing keys (exactly
    /// Python `update` + `HashMap::extend`). The receiver is marked
    /// mutable. Rust/Ruchy emit
    /// `<dict>.extend((<other>).iter().map(|(__k, __v)| (__k.clone(), __v.clone())));`
    /// — cloning each entry keeps `other` usable afterwards (Python
    /// `update` does not consume its argument). Lean refuses (in-place
    /// mutation, same gap as `ListAppend`).
    DictUpdate { dict_name: String, other: Expr },
    /// Positional list insertion — Python `xs.insert(i, x)`. PMAT-502ar
    /// (Tranche 2). Inserts `elem` before index `index`, shifting the
    /// tail right; the receiver is marked mutable. Rust/Ruchy emit
    /// `<list>.insert((<index>) as usize, <elem>);` (same `as usize`
    /// coercion as [`Stmt::IndexAssign`]). First cut covers the in-range
    /// non-negative index (`0 <= i <= len`, matching `Vec::insert`);
    /// Python's negative-index and past-the-end clamping semantics are a
    /// deferred follow-up (same disposition as the negative read-index
    /// slice PMAT-502s). Lean refuses (in-place mutation, same gap as
    /// `ListAppend`).
    ListInsert {
        list_name: String,
        index: Expr,
        elem: Expr,
    },
    /// `for var in iter { body }` — Python `for x in xs:` over a
    /// non-range iterable. PMAT-458, v0.2.0 Track 1.B.
    ///
    /// The frontend reserves `range(...)`-shaped iters for the
    /// existing Let+While desugaring (PMAT-007); ForEach is for
    /// list-typed (and later other collection-typed) iter
    /// expressions. The `elem_ty` is the inferred element type of
    /// `iter`, threaded so the backend knows what to bind `var` to.
    ///
    /// Backends:
    ///   * Rust / Ruchy: `for var in iter.iter().cloned() { body }`
    ///   * Lean: refuses at v0.2.0 first cut — Lean iteration without
    ///     `partial def` machinery is a v0.3.0 sub-track (Lean has
    ///     no for-loop primitive; `forM` / list-recursion is the
    ///     idiomatic encoding, and the body must be encoded as a
    ///     monadic action).
    ///   * Shell: refuses (already not a Python-flow target).
    ///
    /// Governing contract: `C-XLATE-PY-LIST-TO-VEC` —
    /// iteration_order_preserved Bronze theorem implies the lowered
    /// `for x in vec.iter()` produces the same element sequence as
    /// the source Python iteration.
    ForEach {
        var: String,
        iter: Expr,
        elem_ty: Type,
        body: Vec<Stmt>,
        /// PMAT-472 (R3): when `iter` is a dict, the loop iterates its
        /// **keys** (Python `for k in d:`), so Rust/Ruchy emit
        /// `iter.keys().cloned()` instead of `iter.iter().cloned()`,
        /// and `elem_ty` is the key type. `false` for the list case.
        /// NOTE: HashMap key order is unspecified — a `for k in d:` loop
        /// observes keys in arbitrary order (matching CPython ≥3.7 only
        /// for insertion-order semantics it does NOT yet preserve).
        #[serde(default)]
        over_keys: bool,
    },
    /// Paired-target for-loop — Python `for a, b in enumerate(xs)` /
    /// `for a, b in zip(xs, ys)`. PMAT-495 (sprint). A separate variant
    /// from [`Stmt::ForEach`] so its tuple target + iterator-adapter emit
    /// (`.enumerate()` / `.zip()`) don't complicate the single-var path.
    /// Rust/Ruchy emit `for (a, b) in <adapter> { body }`; Lean refuses.
    ForEachPair {
        first: String,
        second: String,
        /// The primary list being iterated.
        iter: Expr,
        kind: PairIterKind,
        body: Vec<Stmt>,
    },
    /// `assert cond` / `assert cond, msg` — Python assert. PMAT-009; the
    /// optional `msg` (a `Str` expression) is PMAT-502ao. Lowers to
    /// `assert!(cond);` (no message) or `assert!(cond, "{}", <msg>);` (with
    /// message) in Rust/Ruchy. Lean is skipped (Lean's assertion machinery
    /// requires `Decidable` instances; deferred).
    Assert { cond: Expr, msg: Option<Expr> },
    /// `raise SomeException("message")` — the first decomposed sub-slice of
    /// PMAT-503 (exceptions). The `message` is the exception constructor's
    /// single string argument (an `Expr::LitStr`, an f-string `Concat`, or
    /// any `Type::Str` expression). Lowers to `panic!("{}", <message>)` in
    /// Rust/Ruchy — the diverging `!` type unifies with any function return,
    /// so a `raise` inside a guard clause type-checks. Lean refuses. The
    /// `try/except` catch side and `Result`-typed propagation follow as
    /// their own slices. PMAT-503a.
    Raise { message: Expr },
    /// `program arg1 arg2 ...` — a single shell-command invocation.
    /// PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B: the first shell
    /// variant to land in meta-HIR.
    ///
    /// Produced exclusively by `bashrs-frontend`; consumed exclusively
    /// by `bashrs-backend` at v0.1.0. Other backends (Rust / Ruchy /
    /// Lean / PTX / WGSL) return `CodegenError::Unsupported` when they
    /// encounter it — the explicit-arm dispatch makes the cross-domain
    /// boundary load-bearing rather than implicit.
    ///
    /// PMAT-042: args are `Vec<Expr>` — every arg is an `Expr::LitStr`
    /// (the unquoted / raw form) at the bashrs-frontend output by
    /// default. `Expr::QuotedString` carries an explicit
    /// `QuotingStrategy` for args that need shell-level quoting.
    /// Future Layer B variants (`Expr::ShellVar`, `Expr::CommandSubstitution`)
    /// plug in here without further IR churn.
    ///
    /// `Stmt::Pipeline { stages: Vec<Stmt::Cmd> }` shipped in PMAT-041.
    Cmd { program: String, args: Vec<Expr> },
    /// `cmd1 | cmd2 | cmd3 …` — POSIX pipeline composition. PMAT-041 /
    /// XPILE-BASHRS-MERGER-001 Layer B (second variant). Each stage
    /// is a `Stmt` so the variant composes (in principle) with the
    /// future `Stmt::ShellLoop` etc. — at v0.1.0 every stage is a
    /// `Stmt::Cmd` in practice; the bashrs-frontend parser rejects
    /// nested-pipeline / control-flow stages with an explicit
    /// diagnostic.
    ///
    /// Same cross-cutting posture as `Stmt::Cmd`: produced only by
    /// `bashrs-frontend`, consumed only by `bashrs-backend`, refused
    /// by every other backend via `Unsupported(...)` arms naming
    /// `C-BASHRS-POSIX-IDEMPOTENCE`.
    Pipeline { stages: Vec<Stmt> },
    /// `NAME=value` — POSIX shell variable assignment. PMAT-051 /
    /// XPILE-BASHRS-MERGER-001 Layer B (assignment idiom).
    ///
    /// The `name` is a POSIX-legal identifier (alphanumeric +
    /// underscore, no leading digit); the `value` is an `Expr` so
    /// future Layer B Expr variants (`Expr::CommandSubstitution`
    /// from PMAT-047/050, etc.) compose naturally — e.g.,
    /// `TODAY=$(date)` → `Stmt::ShellAssign { name: "TODAY", value:
    /// Expr::CommandSubstitution(Box::new(Stmt::Cmd { … })) }`.
    ///
    /// Same cross-domain disposition as the other bashrs variants:
    /// produced only by bashrs-frontend, consumed only by
    /// bashrs-backend, refused by other backends with
    /// `Unsupported(...)` naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    ShellAssign { name: String, value: Expr },
    /// POSIX shell control-flow loop (`for x in …; do … done` /
    /// `while [ … ]; do … done` / `until [ … ]; do … done`). PMAT-048
    /// / XPILE-BASHRS-MERGER-001 Layer B (last variant from the
    /// spec table).
    ///
    /// IR-shape only at v0.1.0 — same scaffold posture as PMAT-046
    /// (Type variants) and PMAT-047 (`Expr::CommandSubstitution`):
    /// the variant is reachable through the IR, the bashrs-backend
    /// can render it, but bashrs-frontend's hand-rolled parser
    /// doesn't produce it yet. The v0.2.0 source fold's real bashrs
    /// parser produces it from real shell input.
    ///
    /// Same cross-domain disposition: bashrs-only; other backends
    /// refuse via `Unsupported(...)` naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    ShellLoop { kind: LoopKind, body: Vec<Stmt> },
}

/// POSIX shell loop dialects. PMAT-048 / XPILE-BASHRS-MERGER-001
/// Layer B. Each variant carries the loop's control predicate /
/// item list; the body lives in `Stmt::ShellLoop::body`.
///
/// Future variants the spec hints at: `Select { var, items }` for
/// `select x in …;` interactive menus (rare; deferred).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LoopKind {
    /// `for VAR in item1 item2 …; do … done`. Each item is an
    /// `Expr` so future Layer B variants
    /// (`Expr::ShellVar` / `Expr::CommandSubstitution` / etc.)
    /// compose here without IR churn.
    For { var: String, items: Vec<Expr> },
    /// `while [ cond ]; do … done`. The condition is an `Expr` —
    /// typically a `Stmt::Cmd`-equivalent test expression, modelled
    /// as `Expr` for uniformity with the rest of the IR. A future
    /// `Expr::ShellTest` variant could carry POSIX `[ … ]` semantics
    /// explicitly; at v0.1.0 the condition is opaque.
    While { cond: Expr },
    /// `until [ cond ]; do … done` — POSIX's inverted while
    /// (continue while cond is *false*).
    Until { cond: Expr },
}

/// Convenience: a single-expression body wraps as `Block { stmts: vec![], trailing_return: expr }`.
/// Useful in tests and for tiny frontends that don't synthesize `let`s.
impl From<Expr> for Block {
    fn from(expr: Expr) -> Self {
        Block {
            stmts: Vec::new(),
            trailing_return: expr,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    /// PMAT-460 (v0.2.0 Track 1.B): set to `true` when the function
    /// mutates this parameter in-place (currently only via
    /// `xs.append(...)`). Rust/Ruchy backends emit `mut name: T` so
    /// the `.push()` call type-checks. Lean ignores (in-place
    /// mutation isn't supported there at v0.2.0).
    #[serde(default)]
    pub mutable: bool,
}

/// MVP type lattice. Will grow as frontends demand it. Keep semantically
/// minimal; map source-language type idiosyncrasies (Python int bigint
/// promotion, C signed/unsigned, etc.) at the frontend boundary.
/// PMAT-455 — `Type` is no longer `Copy` because `Type::List(Box<Type>)`
/// is heap-allocated. Callers that previously relied on `*ty` to
/// dereference / copy now use `.clone()` (cheap — most Type values
/// are leaf variants or 1-deep boxes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Type {
    /// 64-bit signed integer. The fast path for Python `int` — covers
    /// every case where the frontend can prove the value fits.
    I64,
    /// Boolean — produced by comparison ops in [`Expr::BinOp`].
    Bool,
    /// IEEE-754 double — Python `float`. PMAT-477 (R8). Rust/Ruchy emit
    /// `f64`; Lean emits `Float`. Arithmetic (`Expr::FloatBinOp`) is
    /// plain infix (no overflow/checked path — floats saturate to
    /// ±inf, they don't wrap). Comparisons reuse `Expr::BinOp` (plain
    /// infix `<`/`==`/… already type-correct for `f64`, yielding
    /// `Bool`). No governing contract yet (capability-ahead-of-
    /// contract); a `C-PY-FLOAT-ARITH` substrate is queued.
    F64,
    /// Unbounded integer — Python `int`'s native shape. The slow path
    /// of contract `C-PY-INT-ARITH`. Rust/Ruchy emit
    /// `xpile_bigint::BigInt`; Lean emits `Int` (which is already
    /// unbounded). PMAT-012.
    BigInt,
    /// Owned UTF-8 string — Python `str`'s native shape. PMAT-449,
    /// the v0.2.0 Track 1.A foundation
    /// ([`sub/v0.2.0-depyler-merger.md`](../../docs/specifications/sub/v0.2.0-depyler-merger.md)).
    ///
    /// Distinct from [`Type::ShellString`] (which carries an implicit
    /// quoting strategy and bashrs-domain semantics). `Type::Str`
    /// values are UTF-8 owned strings — Rust/Ruchy emit `String`,
    /// Lean emits `String`. v0.2.0 starts with owned-only; `&str`
    /// borrowing is the stretch sub-track 1.D.
    ///
    /// Currently produced by `depyler-frontend` for Python `str`
    /// annotations and `"..."` literals. Other frontends are free to
    /// produce it when they wire up real string parsing.
    ///
    /// Governing contract (Layer 2 translation, code lane): the v0.2.0
    /// `C-XLATE-PY-STR-TO-RUST-STRING` contract once authored.
    Str,
    /// Typed dictionary / map — Python `dict[K, V]`. PMAT-462,
    /// v0.2.0 Track 1.C foundation
    /// ([`sub/v0.2.0-depyler-merger.md`](../../docs/specifications/sub/v0.2.0-depyler-merger.md)
    /// Track 1.C).
    ///
    /// `Type::Dict(Box<Type>, Box<Type>)` represents Python
    /// `dict[K, V]` and lowers to Rust/Ruchy `HashMap<K, V>` and
    /// Lean `List (K × V)` (first cut — Lean's `Std.HashMap` is a
    /// v0.3.0+ refinement once iteration / lookup encoding lands).
    /// Heterogeneous keys / values are not supported at v0.2.0 —
    /// they'd require a `Box<dyn Any>`-style boxing rejected by
    /// the soon-to-be-authored `C-XLATE-PY-DICT-TO-HASHMAP`
    /// contract's `homogeneous_keys_values_rejected` equation.
    ///
    /// At v0.2.0 first cut both `K` and `V` are restricted to
    /// `Type::I64`, `Type::Bool`, or `Type::Str`; nested dicts /
    /// lists as values are subsequent sub-tracks.
    ///
    /// Governing contract: `C-XLATE-PY-DICT-TO-HASHMAP` (to be
    /// authored alongside this PR's substrate ratchet).
    Dict(Box<Type>, Box<Type>),
    /// Homogeneous list / vector. PMAT-455, v0.2.0 Track 1.B
    /// foundation (per [`sub/v0.2.0-depyler-merger.md`](../../docs/specifications/sub/v0.2.0-depyler-merger.md)).
    ///
    /// `Type::List(Box<Type>)` represents Python `list[T]` and lowers
    /// to Rust/Ruchy `Vec<T>` (owned, length-stable) and Lean `List T`.
    /// Heterogeneous lists are not supported at v0.2.0 — they'd
    /// require either `Box<dyn Any>`-style boxing (rejected by
    /// `C-XLATE-PY-LIST-TO-VEC`'s `heterogeneous_list_rejected`
    /// equation) or a Silver-tier sum type which is post-v0.2.0.
    ///
    /// At v0.2.0 first cut the element type is restricted to
    /// `Type::I64`; subsequent sub-tracks widen to `Type::Str`,
    /// `Type::Bool`, and nested `Type::List`. Other element types
    /// are explicitly rejected by the frontend with a clear error.
    ///
    /// Governing contract: `C-XLATE-PY-LIST-TO-VEC` (already QUORUM
    /// at depth-13).
    List(Box<Type>),
    /// `Type::Set(Box<Type>)` represents Python `set[T]` and lowers to a
    /// Rust/Ruchy `std::collections::HashSet<T>`. PMAT-500 (Tranche 2),
    /// read-side first cut (literal + membership). Lean refuses.
    Set(Box<Type>),
    /// `Type::Tuple(Vec<Type>)` represents Python `tuple[T0, T1, ...]` and
    /// lowers to a Rust/Ruchy anonymous tuple `(T0, T1, ...)`. PMAT-494
    /// (sprint), first cut: fixed-arity heterogeneous tuples in return /
    /// expression position (multiple return `return a, b`). Lean refuses
    /// at first cut (Prod encoding deferred). Tuple *unpacking* (`a, b =
    /// f()`) is a follow-up slice.
    Tuple(Vec<Type>),
    /// POSIX shell string — quoted-aware string type for the bashrs
    /// domain. PMAT-046 / XPILE-BASHRS-MERGER-001 Layer B.
    ///
    /// Distinct from a generic `String` (which xpile doesn't have at
    /// v0.1.0): a `ShellString` value semantically carries an
    /// implicit `QuotingStrategy` and a shellcheck-equivalent
    /// validity claim. The v0.1.0 type is *load-bearing for future
    /// signatures* — when bashrs-frontend gains a real parser that
    /// types shell variables, it'll annotate them as
    /// `Type::ShellString`. Until then this variant is unused at the
    /// surface but present in the IR for the Bronze→Silver
    /// refinement of `C-BASHRS-POSIX-IDEMPOTENCE` (the typed POSIX
    /// state needed in `contracts/lean/Bashrs.lean` to model
    /// concrete shell semantics, not just abstract Outcomes).
    ///
    /// Other backends refuse `Type::ShellString` via `Unsupported`
    /// arms naming the bashrs contract.
    ShellString,
    /// POSIX exit code (i32 range, conventionally 0..=255). PMAT-046
    /// / XPILE-BASHRS-MERGER-001 Layer B.
    ///
    /// Same posture as `ShellString` — present for future signatures
    /// where shell-domain functions need a typed exit status (the
    /// Silver-tier Lean model's `Outcome` will carry an `ExitCode`
    /// field rather than just an opaque `observable: String`).
    ///
    /// Other backends refuse `Type::ExitCode` via `Unsupported` arms.
    ExitCode,
    /// The unit type — Python `None` as a return annotation (a void
    /// function). PMAT-502bl (Tranche 2). Rust/Ruchy emit `()`; Lean
    /// refuses (a side-effecting void function has no total-function
    /// encoding). Carried only by `Function::return_type`.
    Unit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// The unit value `()` — the trailing "return" of a void function
    /// (Python `-> None`). PMAT-502bl (Tranche 2). Rust/Ruchy emit `()`;
    /// Lean refuses. Types as [`Type::Unit`].
    Unit,
    /// Local identifier reference (function parameter or future `let`).
    Ident(String),
    /// Integer literal, lowered as i64 at the boundary.
    LitInt(i64),
    /// IEEE-754 float literal — Python `3.14`. PMAT-477 (R8).
    /// Rust/Ruchy emit `<v>f64`; Lean emits the decimal as a `Float`.
    LitFloat(f64),
    /// Float arithmetic `a <op> b` for `f64` operands — Python `+ - *
    /// /` on floats. PMAT-477 (R8). Distinct from [`Expr::BinOp`]
    /// because float arithmetic is **plain infix** (no `checked_*` /
    /// overflow path — IEEE-754 saturates to ±inf), and `/` is true
    /// division, not floor. Float comparisons stay on `Expr::BinOp`
    /// (their plain-infix emission is already `f64`-correct → `Bool`).
    FloatBinOp {
        op: FloatOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// Boolean literal — Python `True` / `False`. PMAT-456,
    /// v0.2.0 Track 1.B. Rust/Ruchy emit `true` / `false`; Lean
    /// emits `True` / `False` (capitalised).
    LitBool(bool),
    /// Binary operation. Type inference is intentionally absent at v0.1.0:
    /// each backend infers result type from operand types via [`BinOp`].
    BinOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `len(collection)` — Python builtin returning the length of a
    /// list / str / dict / etc. PMAT-459, v0.2.0 Track 1.B.
    ///
    /// At v0.2.0 first cut the collection must type as `Type::List(_)`
    /// or `Type::Str` — both have well-defined byte-/element- counts.
    /// The result is `Type::I64` (matching Python's signed int return
    /// from len).
    ///
    /// Backends:
    ///   * Rust / Ruchy: `<collection>.len() as i64` — `.len()` returns
    ///     `usize`, cast back to `i64` for the Python int domain.
    ///   * Lean: `(<collection>.length : Int)` for List; `<s>.length`
    ///     for String (Lean's `String.length` is already `Nat`,
    ///     coerced to `Int` via the explicit type ascription).
    ///   * Shell: refuses (no collection-length concept).
    Len(Box<Expr>),
    /// List indexed access — Python `xs[i]`. PMAT-457, v0.2.0 Track 1.B.
    ///
    /// At v0.2.0 first cut the `index` expression is treated as a
    /// non-negative `i64` and the backend coerces to `usize` (Rust /
    /// Ruchy) or `Nat` (Lean) at emission. Negative-index semantics
    /// (Python's `xs[-1]` = last element) are not supported — the
    /// emitted Rust will panic on overflow if `index < 0`.
    /// Out-of-range access panics in all three backends, matching
    /// Python's `IndexError` posture (Rust: panic in `vec[i]`;
    /// Lean: `xs[i]!` panic on `none`).
    ///
    /// Governing contract: `C-XLATE-PY-LIST-TO-VEC` —
    /// `iteration_order_preserved` Bronze theorem implies the lowered
    /// `Vec`'s `[i]` produces the same element as the source `list`'s
    /// `[i]`.
    Index {
        collection: Box<Expr>,
        index: Box<Expr>,
    },
    /// Bounded slice — Python `xs[lo:hi]`. PMAT-496 (sprint), first cut:
    /// both bounds present, non-negative `i64`, step 1. `of_str` (set by
    /// the frontend from the collection's type) selects the emit shape —
    /// list (`of_str=false`) emits `<c>[<lo> as usize..<hi> as usize]
    /// .to_vec()`; str (`of_str=true`) emits the same range with
    /// `.to_string()`. Result types as the collection's type (List(T) →
    /// List(T); Str → Str). NOTE str slicing is byte-indexed
    /// (ASCII-correct; a non-char-boundary index panics, matching the
    /// existing str byte-length posture). PMAT-502r: `lo`/`hi` are
    /// `Option` — an absent bound is an open end (`xs[a:]`, `xs[:b]`,
    /// `xs[:]`), emitting a half-open / full Rust range (`a..`, `..b`,
    /// `..`). PMAT-502bc: `step` carries a **positive integer literal**
    /// step over a *list* (`xs[a:b:c]`, `xs[::2]`, …); `None` is the
    /// default step of 1. With a step, the list emit becomes
    /// `<c>[<range>].iter().step_by(<step>).cloned().collect::<Vec<_>>()`.
    /// The `xs[::-1]` reverse idiom is lowered to [`Expr::Reversed`]
    /// upstream, so a negative `step` never reaches here; other negative
    /// steps and stepped *string* slices remain deferred. Lean refuses.
    Slice {
        collection: Box<Expr>,
        lo: Option<Box<Expr>>,
        hi: Option<Box<Expr>>,
        of_str: bool,
        step: Option<i64>,
    },
    /// Dictionary literal — Python `{"a": 1, "b": 2}`. PMAT-462,
    /// v0.2.0 Track 1.C foundation.
    ///
    /// All keys and values must type identically; heterogeneous
    /// literals are rejected at frontend lowering time. Empty
    /// literal `{}` requires an annotation upstream; v0.2.0 first
    /// cut requires non-empty.
    ///
    /// Backends:
    ///   * Rust / Ruchy: emit `{ let mut m = HashMap::new(); m.insert(k, v); ... m }`
    ///     as a block expression returning the owned HashMap.
    ///   * Lean: emit `[(k, v), ...]` (a List of pairs — first cut
    ///     before Std.HashMap encoding lands).
    ///   * Shell: refuses.
    ///
    /// PMAT-466 (v0.2.0 Track 1.C operations): the empty literal
    /// `{}` is now permitted as `DictLit(vec![])` when the binding
    /// site carries a `dict[K, V]` annotation (the frontend threads
    /// the K/V from the annotation). Rust/Ruchy emit
    /// `std::collections::HashMap::new()` (type inferred from the
    /// `let` annotation); Lean emits `[]`.
    DictLit(Vec<(Expr, Expr)>),
    /// Dictionary indexed read — Python `d[k]`. PMAT-466,
    /// v0.2.0 Track 1.C operations. Distinct from [`Expr::Index`]
    /// (which lowers list `xs[i]` to `Vec` indexing) because the
    /// dict path emits `HashMap` keyed lookup (`d[&k]`), not a
    /// `usize`-coerced positional index. The frontend chooses the
    /// variant by the receiver's inferred type
    /// (`Type::Dict` → `DictGet`, `Type::List` → `Index`).
    ///
    /// Missing-key semantics match Python's `KeyError`: Rust's
    /// `HashMap` `Index` impl panics on an absent key, mirroring the
    /// `vec[i]` out-of-range panic posture already used for lists.
    ///
    /// Backends:
    ///   * Rust / Ruchy: `<dict>[&(<key>)].clone()` — owned value
    ///     (the v0.2.0 owned-only posture), panic on absent key.
    ///   * Lean: refuses at v0.2.0 first cut (the `List (K × V)`
    ///     encoding has no panic-on-absent lookup; deferred to the
    ///     `Std.HashMap` upgrade alongside Lean iteration/mutation).
    ///   * Shell: refuses.
    DictGet { dict: Box<Expr>, key: Box<Expr> },
    /// Dictionary get-with-default — Python `d.get(k, default)`.
    /// PMAT-466, v0.2.0 Track 1.C operations. Total (never panics):
    /// returns the stored value if `k` is present, else `default`.
    ///
    /// Backends:
    ///   * Rust / Ruchy: `<dict>.get(&(<key>)).cloned().unwrap_or(<default>)`.
    ///   * Lean: refuses at v0.2.0 first cut (same reason as
    ///     `DictGet`).
    ///   * Shell: refuses.
    DictGetOr {
        dict: Box<Expr>,
        key: Box<Expr>,
        default: Box<Expr>,
    },
    /// Dictionary key membership — Python `k in d`. PMAT-466,
    /// v0.2.0 Track 1.C operations. Result types as `Type::Bool`.
    ///
    /// Backends:
    ///   * Rust / Ruchy: `<dict>.contains_key(&(<key>))`.
    ///   * Lean: refuses at v0.2.0 first cut.
    ///   * Shell: refuses.
    DictContains { dict: Box<Expr>, key: Box<Expr> },
    /// Dict view — Python `d.keys()` / `d.values()` materialized to a new
    /// `Vec`. PMAT-502v (Tranche 2). Rust/Ruchy emit
    /// `<dict>.keys().cloned().collect::<Vec<_>>()` (or `.values()`). Result
    /// types as `List(K)` (keys) / `List(V)` (values) — so it composes with
    /// `len`/`sum`/`sorted`/for-iteration. HashMap iteration order is
    /// unspecified (callers should not rely on it). Lean refuses. (`.items()`
    /// → `List(Tuple[K,V])` follows as its own slice.)
    DictView { dict: Box<Expr>, kind: DictViewKind },
    /// Set membership — Python `x in s`. PMAT-500 (Tranche 2). Result
    /// types as `Type::Bool`. Rust/Ruchy emit `<set>.contains(&(<elem>))`;
    /// Lean refuses. The frontend chooses this over [`Expr::DictContains`]
    /// by the RHS type (`Type::Set` → `SetContains`, `Type::Dict` →
    /// `DictContains`).
    SetContains { set: Box<Expr>, elem: Box<Expr> },
    /// List membership — Python `x in xs` when `xs` is a `List`. PMAT-502an
    /// (Tranche 2). Result types as `Type::Bool`. Rust/Ruchy emit
    /// `(<list>).contains(&(<elem>))` (the element type is `Eq`). The frontend
    /// chooses this over the set/dict/str membership forms by the RHS type.
    /// Lean refuses.
    ListContains { list: Box<Expr>, elem: Box<Expr> },
    /// String substring containment — Python `needle in haystack` when
    /// `haystack` is a `Str`. PMAT-502o (Tranche 2). Rust/Ruchy emit
    /// `(<haystack>).contains(&(<needle>)[..])`; result types as `Bool`.
    /// The frontend chooses this over [`Expr::SetContains`]/
    /// [`Expr::DictContains`] by the RHS type (`Type::Str`). Lean refuses.
    StrContains {
        haystack: Box<Expr>,
        needle: Box<Expr>,
    },
    /// Homogeneous list literal — Python `[1, 2, 3]`. PMAT-455,
    /// v0.2.0 Track 1.B foundation.
    ///
    /// All elements must type identically; heterogeneous literals are
    /// rejected at frontend lowering time per
    /// `C-XLATE-PY-LIST-TO-VEC::heterogeneous_list_rejected`. Empty
    /// list `[]` requires a type annotation upstream (since the
    /// frontend can't infer the element type from zero elements);
    /// v0.2.0 first cut requires non-empty list literals.
    ///
    /// Backends:
    ///   * Rust / Ruchy emit `vec![<elem>, <elem>, ...]`
    ///   * Lean emits `[<elem>, <elem>, ...]` (Lean's built-in
    ///     `List` literal syntax)
    ///   * Shell refuses (lists aren't a POSIX construct)
    ListLit(Vec<Expr>),
    /// Set literal — Python `{1, 2, 3}`. PMAT-500 (Tranche 2). All
    /// elements type identically. Rust/Ruchy emit a `HashSet`-init block
    /// `{ let mut s = HashSet::new(); s.insert(e); … s }`; Lean refuses.
    /// Result types as [`Type::Set`]. (Empty `set()` / `.add()` mutation
    /// follow as their own slice; `{}` is an empty *dict*, not a set.)
    SetLit(Vec<Expr>),
    /// Set algebra — Python `a | b` (union), `a & b` (intersection),
    /// `a - b` (difference), `a ^ b` (symmetric difference) when **both**
    /// operands are [`Type::Set`]. PMAT-502g (Tranche 2). The frontend
    /// disambiguates from the int bitwise/arith [`Expr::BinOp`] by operand
    /// type. Rust/Ruchy emit `(lhs).union(&(rhs)).cloned().collect::<…>()`
    /// (and `.intersection`/`.difference`/`.symmetric_difference`), yielding
    /// a **new** `HashSet`. Result types as the operand `Set` type. Lean
    /// refuses.
    SetOp {
        lhs: Box<Expr>,
        op: SetOp,
        rhs: Box<Expr>,
    },
    /// Tuple literal — Python `(a, b)` / multiple-return `return a, b`.
    /// PMAT-494 (sprint). Elements may be heterogeneous (unlike
    /// [`Expr::ListLit`]). Rust/Ruchy emit `(e0, e1, ...)`; Lean refuses
    /// at first cut. Result types as [`Type::Tuple`].
    TupleLit(Vec<Expr>),
    /// Tuple constant-index — Python `t[N]` over a `Tuple`-typed `t` with a
    /// compile-time non-negative literal `N`. PMAT-502q (Tranche 2). Rust
    /// tuples use field access (`t.0`), not `[]` indexing, so this is a
    /// distinct node from [`Expr::Index`] (list/dict subscript). Rust/Ruchy
    /// emit `(<tuple>).N.clone()`; result types as the N-th element type.
    /// Lean refuses (tuples unsupported in the Lean lane).
    TupleIndex { tuple: Box<Expr>, index: usize },
    /// String concatenation — Python `str + str` semantics. PMAT-451,
    /// v0.2.0 Track 1.A. Distinct from `BinOp::Add` because:
    ///   * No overflow concept (strings never overflow).
    ///   * Backend host varies: Rust/Ruchy emit `format!("{}{}", l, r)`;
    ///     Lean emits `l ++ r`.
    ///   * The contract substrate's associativity equation
    ///     (`C-XLATE-PY-STR-TO-RUST-STRING::concatenation_associativity`)
    ///     attaches to this variant, not to `BinOp::Add`.
    ///
    /// Produced by depyler-frontend when both operands of `+` are
    /// `Type::Str`-typed. f-string lowering (subsequent sub-track)
    /// generalises to `Vec<Expr>` parts via a `Concat { parts }`
    /// rewrite; the binary form is sufficient for v0.2.0 first cut.
    Concat { lhs: Box<Expr>, rhs: Box<Expr> },
    /// List concatenation — Python `xs + ys` over two lists. PMAT-502bg
    /// (Tranche 2). The companion of [`Expr::Concat`] (string `+`) on the
    /// list side; the frontend chooses it when both `+` operands type as
    /// `Type::List`. Rust/Ruchy emit
    /// `(<lhs>).iter().chain((<rhs>).iter()).cloned().collect::<Vec<_>>()`
    /// — a fresh `Vec` that consumes neither operand (matching Python,
    /// where `+` does not mutate either list). The result types as the
    /// list type. Lean refuses.
    ListConcat { lhs: Box<Expr>, rhs: Box<Expr> },
    /// `"<fmt>".format(args…)` — Python `str.format` with **sequential**
    /// `{}` placeholders. PMAT-502bh (Tranche 2). `fmt` is the (already
    /// validated) format string; its `{}` placeholders map one-to-one to
    /// `args`, and `{{` / `}}` are literal-brace escapes — identical
    /// semantics to Rust's `format!`. Rust/Ruchy emit
    /// `format!("<fmt>", <arg0>, …)` (the `fmt` re-escaped as a Rust
    /// string literal via `{:?}`). First cut: `int` / `str` args only
    /// (a `bool` formats `True`/`False` in Python vs `true`/`false` in
    /// Rust, and a whole-number `float` drops its `.0` in Rust's
    /// `Display` — both deferred). Indexed (`{0}`) / named (`{name}`) /
    /// spec'd (`{:.2f}`) fields are rejected at the frontend. Lean refuses.
    StrFormat { fmt: String, args: Vec<Expr> },
    /// `s[i]` over a **string** — Python returns the 1-char string at index
    /// `i`. PMAT-502cd (Tranche 2). Unlike list `Expr::Index`, Rust `String`
    /// has no positional `[]`, so the backends materialise the chars and
    /// index that: `{ let __cs: Vec<char> = (s).chars().collect(); let __i =
    /// (i); let __idx = if __i < 0 { __cs.len() as i64 + __i } else { __i };
    /// __cs[__idx as usize].to_string() }`. Negative `i` counts from the end
    /// (Python semantics); an out-of-range index panics (≈ `IndexError`).
    /// Result types as `Str`. Lean refuses.
    StrCharAt { string: Box<Expr>, index: Box<Expr> },
    /// The characters of a string as a `list[str]` (each a 1-char string) —
    /// produced by lowering `for c in s` (string iteration). PMAT-502cl
    /// (Tranche 2). Rust/Ruchy emit `(<s>).chars().map(|__c| __c.to_string())
    /// .collect::<Vec<String>>()`; result types as `list[str]`, so the
    /// enclosing `Stmt::ForEach`'s `.iter().cloned()` yields `String` items.
    /// Lean refuses.
    StrChars { string: Box<Expr> },
    /// `ord(c)` — the Unicode code point of a 1-char string (→ `int`).
    /// PMAT-502cm (Tranche 2). Rust/Ruchy emit `((<c>).chars().next()
    /// .expect("…") as i64)`. Lean refuses.
    Ord { value: Box<Expr> },
    /// `chr(n)` — the 1-char string for a code point (→ `str`). PMAT-502cm.
    /// Rust/Ruchy emit `char::from_u32((<n>) as u32).expect("…").to_string()`
    /// (an out-of-range code point panics, ≈ Python's `ValueError`). Lean
    /// refuses.
    Chr { value: Box<Expr> },
    /// A formatted f-string field — Python `{value:spec}` where `spec` is a
    /// static format spec (e.g. `.2f`, `05d`, `>10`). PMAT-502am (Tranche 2).
    /// `rust_spec` is the already-translated Rust format spec (the frontend
    /// maps the supported Python subset to it). Rust/Ruchy emit
    /// `format!("{:<rust_spec>}", <value>)` → `Str`. Lean refuses.
    FormatSpec { value: Box<Expr>, rust_spec: String },
    /// No-argument Python string transform method — `s.upper()` /
    /// `s.lower()` / `s.strip()`. PMAT-492 (sprint). Result types as
    /// `Type::Str`. Distinct from [`Expr::Call`] (a free function
    /// `callee(args)`) because these are *receiver methods* with bespoke
    /// per-backend lowering. The receiver must type as `Type::Str`.
    ///
    /// Backends:
    ///   * Rust / Ruchy: `Upper` → `<recv>.to_uppercase()`, `Lower` →
    ///     `<recv>.to_lowercase()`, `Strip` → `<recv>.trim().to_string()`.
    ///   * Lean: refuses at first cut (string-method encoding deferred,
    ///     alongside the other str-domain refusals).
    ///   * Shell: refuses.
    ///
    /// `args` carries method arguments (empty for the no-arg transforms;
    /// one pattern expr for `StartsWith`/`EndsWith`). `split`/`join`
    /// list-interplay still follows as its own slice.
    StrMethod {
        recv: Box<Expr>,
        op: StrMethodOp,
        args: Vec<Expr>,
    },
    /// Conditional expression — Python's `then if cond else else_`, lowered
    /// from `ast::Expr::IfExp`. Both branches must produce the same type at
    /// v0.1.0 (the frontend rejects branch-type-mismatch; future versions
    /// may unify or pick a least upper bound).
    IfExpr {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    /// Direct call by name: `callee(args...)`. The callee must be a
    /// top-level identifier (no method calls, no first-class function
    /// values at v0.1.0). Result type is inferred as I64 — the frontend
    /// lacks a cross-function signature table, so promote to proper
    /// inference when one lands.
    Call { callee: String, args: Vec<Expr> },
    /// Scalar numeric builtin — Python `abs(x)` / `min(a, b)` /
    /// `max(a, b)`. PMAT-498 (Tranche 2). Rust/Ruchy emit the receiver-
    /// method form (`(a).abs()` / `(a).min(b)` / `(a).max(b)`), valid for
    /// both `i64` and `f64`. Result types as the first arg's type. Lean
    /// refuses at first cut. (`sum`/1-arg `min`/`max` over a list need an
    /// element-type hint and follow as their own slice.)
    NumBuiltin { op: NumBuiltinOp, args: Vec<Expr> },
    /// `sum(xs)` over a numeric list — Python builtin. PMAT-498b
    /// (Tranche 2). Rust/Ruchy emit `<list>.iter().sum::<T>()` with the
    /// turbofish `T` selected by `of_float` (the frontend sets it from
    /// the element type — `i64` for `list[int]`, `f64` for `list[float]`).
    /// Result types as the element type. Lean refuses.
    Sum { list: Box<Expr>, of_float: bool },
    /// `all(xs)` / `any(xs)` over a `list[bool]` — Python builtins.
    /// PMAT-502j (Tranche 2). Rust/Ruchy emit
    /// `<list>.iter().all(|&__b| __b)` (or `.any(…)`); result types as
    /// `Bool`. Like Python, `all([])` is `true` and `any([])` is `false`
    /// (the iterator-adaptor identities). Lean refuses.
    BoolReduce { list: Box<Expr>, is_all: bool },
    /// Sequence repetition — Python `seq * n` / `n * seq` where `seq` is a
    /// `Str` or `List` and `n` an `Int`. PMAT-502k (Tranche 2). Rust/Ruchy
    /// emit `(<seq>).repeat(((<n>).max(0)) as usize)` — one form covers both
    /// `str::repeat` (→ `String`) and slice `<[T]>::repeat` (→ `Vec<T>`).
    /// The `.max(0)` clamps a negative count to the empty sequence, matching
    /// Python (`"x" * -1 == ""`). Result types as `seq`. Lean refuses.
    Repeat { seq: Box<Expr>, n: Box<Expr> },
    /// Numeric conversion — Python `int(x)` / `float(x)`. PMAT-502m
    /// (Tranche 2). For a **numeric** `value` (`from_str = false`),
    /// Rust/Ruchy emit `((<value>) as i64)` (for `int`, which truncates
    /// toward zero exactly like Python) or `((<value>) as f64)` (for
    /// `float`). PMAT-502bf: for a **string** `value` (`from_str = true`),
    /// they emit `(<value>).trim().parse::<i64>().expect(…)` /
    /// `…parse::<f64>().expect(…)` — `.trim()` matches Python's
    /// whitespace stripping, and a parse failure panics, matching Python's
    /// `ValueError`. Result types as `I64`/`F64` per `to_float`. Lean
    /// refuses.
    NumCast {
        value: Box<Expr>,
        to_float: bool,
        #[serde(default)]
        from_str: bool,
    },
    /// Python `str(x)` over an **int** or **float** `x` → its string form.
    /// PMAT-502ad (int); `of_float` is PMAT-502af. For int, Rust/Ruchy emit
    /// `format!("{}", <value>)`. For float they emit a block that matches
    /// Python's formatting (`is_nan()` → `"nan"`; finite whole numbers get a
    /// `".0"` suffix; otherwise `format!("{}", …)`), since Rust's bare
    /// `format!` prints e.g. `2.0` as `"2"`. Result types as `Str`.
    /// (`str(bool)` desugars to an `IfExpr`, PMAT-502ae.) Lean refuses.
    ToStr { value: Box<Expr>, of_float: bool },
    /// `round(x)` over a **float** `x` → the nearest integer (**Int**).
    /// PMAT-502ak (Tranche 2). Rust/Ruchy emit `((<value>).round_ties_even()
    /// as i64)` — `round_ties_even` is round-half-to-**even** (banker's
    /// rounding), exactly matching Python's `round` (e.g. `round(2.5) == 2`,
    /// `round(3.5) == 4`), unlike Rust's `f64::round` (half-away-from-zero).
    /// `round(int)` is the identity (handled in the frontend without this
    /// node); the 2-arg `round(x, n)` form follows. Lean refuses.
    RoundToInt { value: Box<Expr> },
    /// `round(x, n)` over a **float** `x` and **int** `n` → the float rounded
    /// to `n` decimal places (**Float**). PMAT-502al (Tranche 2). Rust/Ruchy
    /// emit a block that, for `n >= 0`, formats to `n` decimals and parses
    /// back (`format!("{:.1$}", x, n).parse()`) — Rust's float formatting is
    /// round-half-to-**even**, the same correct decimal rounding Python uses,
    /// so it matches Python exactly (incl. `round(2.675, 2) == 2.67` from the
    /// float repr). For `n < 0` it scales down, `round_ties_even`s, and scales
    /// back. Lean refuses.
    RoundToDigits {
        value: Box<Expr>,
        ndigits: Box<Expr>,
    },
    /// `sorted(xs)` / `sorted(xs, reverse=True)` / `sorted(xs, key=lambda
    /// p: e)` over a list — Python builtin returning a **new** sorted list
    /// (the input is not mutated). PMAT-502c; `reverse` is PMAT-502f; the
    /// optional `key` lambda is PMAT-502z. Rust/Ruchy emit
    /// `{ let mut __v = <list>.clone(); __v.sort(); __v }` (ascending) or,
    /// when `reverse`, append `__v.reverse();`. With a `key`, emit
    /// `__v.sort_by_key(|__k| { let <param> = __k.clone(); <body> })` — the
    /// clone-to-local binds the element by value so the body type-checks
    /// regardless of `sort_by_key`'s `&T` argument; the body must yield an
    /// `Ord` key. Result types as the list's type. Lean refuses.
    Sorted {
        list: Box<Expr>,
        reverse: bool,
        key: Option<SortKey>,
    },
    /// `list(reversed(xs))` / `reversed(xs)` over a list — Python builtin
    /// returning a **new** reversed list (the input is not mutated).
    /// PMAT-502d (Tranche 2). Rust/Ruchy emit
    /// `{ let mut __v = <list>.clone(); __v.reverse(); __v }`; result types
    /// as the list's type. Lean refuses. (Python's `reversed` yields a lazy
    /// iterator, but the supported subset materializes it as a `Vec`.)
    Reversed { list: Box<Expr> },
    /// `list(range(start, stop, step))` — materialise a range into a `Vec`.
    /// PMAT-502cj (Tranche 2). Rust/Ruchy emit `((<start>)..(<stop>))
    /// .collect::<Vec<i64>>()` for `step == 1`, or `.step_by(<step> as usize)`
    /// before `collect` for `step > 1`. Result types as `list[int]`. The
    /// frontend admits a positive literal step only (negative step / non-int
    /// bounds deferred). Lean refuses.
    RangeList {
        start: Box<Expr>,
        stop: Box<Expr>,
        step: i64,
    },
    /// `filter(lambda p: pred, xs)` over a list — Python builtin. PMAT-502ab
    /// (Tranche 2). The supported subset materializes the lazy `filter`
    /// iterator as a `Vec`. The `lambda` is a [`SortKey`] (param + body) whose
    /// body is a `Bool` predicate. Rust/Ruchy emit
    /// `<list>.iter().cloned().filter(|__k| { let p = __k.clone(); pred })
    /// .collect::<Vec<_>>()`; result types as the **input** list type
    /// (filter keeps the element type, drops some elements). Lean refuses.
    Filter { list: Box<Expr>, lambda: SortKey },
    /// `map(lambda p: e, xs)` over a list — Python builtin. PMAT-502ac
    /// (Tranche 2). The supported subset materializes the lazy `map`
    /// iterator as a `Vec`. The `lambda` is a [`SortKey`] (param + body).
    /// Rust/Ruchy emit `<list>.iter().cloned().map(|__k| { let p =
    /// __k.clone(); e }).collect::<Vec<_>>()`; result types as
    /// `List(<body type>)` — the body's transformed element type (correct
    /// for arithmetic / `len` / conversion bodies, which is what lowers).
    /// Lean refuses.
    Map { list: Box<Expr>, lambda: SortKey },
    /// `enumerate(xs)` over a list — Python builtin, materialized to a `Vec`
    /// of `(index, element)` 2-tuples. PMAT-502ai (Tranche 2). Rust/Ruchy
    /// emit `<list>.iter().cloned().enumerate().map(|(__i, __e)| (__i as i64,
    /// __e)).collect::<Vec<_>>()`; result types as `List(Tuple[I64, elem])`.
    /// (`enumerate(xs, start)` follows.) Lean refuses.
    Enumerate { list: Box<Expr> },
    /// `zip(xs, ys)` over two lists — Python builtin, materialized to a `Vec`
    /// of paired 2-tuples (truncated to the shorter). PMAT-502ai (Tranche 2).
    /// Rust/Ruchy emit `<left>.iter().cloned().zip(<right>.iter().cloned())
    /// .collect::<Vec<_>>()`; result types as `List(Tuple[elemL, elemR])`.
    /// Lean refuses.
    Zip { left: Box<Expr>, right: Box<Expr> },
    /// `min(xs)` / `max(xs)` over a list — the 1-arg reduction form of
    /// the Python builtins (distinct from the 2-arg `min(a, b)` which is
    /// an [`Expr::NumBuiltin`]). PMAT-502e (`list[int]`); `of_float` is
    /// PMAT-502h (`list[float]`). For `list[int]` Rust/Ruchy emit
    /// `<list>.iter().copied().min().unwrap()` (or `.max()`) — `i64: Ord`.
    /// For `list[float]` they emit `<list>.iter().copied().fold(f64::INFINITY,
    /// f64::min)` (or `f64::NEG_INFINITY, f64::max`) since `f64` lacks `Ord`.
    /// Result types as the list's element type. Lean refuses. For `int` an
    /// empty list panics (`.unwrap()`, ~Python's `ValueError`); for `float`
    /// it yields ±∞ (the fold identity — a first-cut wart on empty input).
    /// PMAT-502aa: an optional `key=lambda p: e` reduces by the key instead
    /// of the element. With a key, Rust/Ruchy emit
    /// `<list>.iter().cloned().min_by_key(|__k| { let p = __k.clone(); e })
    /// .unwrap()` (or `max_by_key`); the element can be any type (only the
    /// key needs `Ord`), and the result is still the **element**, not the key.
    ListMinMax {
        list: Box<Expr>,
        is_max: bool,
        of_float: bool,
        key: Option<SortKey>,
    },
    /// List query method — Python `xs.count(x)` / `xs.index(x)` over a
    /// `list[int]`. PMAT-502u (Tranche 2). Both return **Int**. Rust/Ruchy
    /// emit `<list>.iter().filter(|&&__e| __e == <arg>).count() as i64`
    /// (count) and
    /// `<list>.iter().position(|&__e| __e == <arg>).map(|__i| __i as i64)
    /// .expect(…)` (index — panics if absent, matching Python `ValueError`).
    /// First cut is `list[int]` (`Copy`+`Eq`). Lean refuses.
    ListQuery {
        list: Box<Expr>,
        op: ListQueryOp,
        arg: Box<Expr>,
    },
    /// List pop — Python `xs.pop()` / `xs.pop(i)`. PMAT-502as (Tranche 2).
    /// An *expression* that removes an element from the receiver and
    /// evaluates to it (so the receiver must be mutable; the frontend
    /// marks it). With no `index`, removes and returns the **last**
    /// element; with an `index`, removes and returns the element at that
    /// position. Both panic when out of range, matching Python's
    /// `IndexError`. The result type is the list's element type.
    ///
    /// Backends:
    ///   * Rust / Ruchy: no index → `(<list>).pop().unwrap()`; with index
    ///     → `(<list>).remove((<index>) as usize)`.
    ///   * Lean: refuses (in-place mutation, same gap as `Stmt::ListAppend`).
    ListPop {
        list: Box<Expr>,
        index: Option<Box<Expr>>,
    },
    /// Dict pop — Python `d.pop(k)` / `d.pop(k, default)`. PMAT-502au
    /// (Tranche 2). An *expression* that removes the entry for `key` and
    /// evaluates to its value (so the receiver must be mutable; the
    /// frontend marks it). With no `default`, panics when the key is
    /// absent, matching Python's `KeyError`; with a `default`, evaluates
    /// to it instead. The result type is the dict's value type.
    ///
    /// Backends:
    ///   * Rust / Ruchy: no default → `(<dict>).remove(&(<key>)).unwrap()`;
    ///     with default → `(<dict>).remove(&(<key>)).unwrap_or(<default>)`.
    ///   * Lean: refuses (in-place mutation, same gap as `Stmt::ListAppend`).
    DictPop {
        dict: Box<Expr>,
        key: Box<Expr>,
        default: Option<Box<Expr>>,
    },
    /// Dict get-or-insert — Python `d.setdefault(k, default)`. PMAT-502ax
    /// (Tranche 2). An *expression*: if `key` is present, evaluates to its
    /// value; otherwise inserts `default` under `key` and evaluates to it.
    /// Because the absent case mutates, the receiver must be mutable (the
    /// frontend marks it). The result type is the dict's value type.
    ///
    /// Backends:
    ///   * Rust / Ruchy: `(<dict>).entry(<key>.clone()).or_insert(<default>).clone()`
    ///     — `.entry` consumes the key, so it is `.clone()`d to keep the
    ///     caller's binding usable (a no-op move for `Copy` keys); the
    ///     trailing `.clone()` lifts the `&mut V` to an owned value.
    ///   * Lean: refuses (in-place mutation, same gap as `Stmt::ListAppend`).
    DictSetDefault {
        dict: Box<Expr>,
        key: Box<Expr>,
        default: Box<Expr>,
    },
    /// Unary operation — `not x` (logical, Bool → Bool) or `-x`
    /// (numeric negate, I64 → I64).
    UnOp { op: UnOp, operand: Box<Expr> },
    /// String literal — the unquoted / "raw token" form. PMAT-042 /
    /// XPILE-BASHRS-MERGER-001 Layer B (Expr-side).
    ///
    /// Used exclusively in `Stmt::Cmd::args` and pipeline stages
    /// (bashrs domain); the Python / C / Rust frontends don't
    /// produce strings at v0.1.0 because their meta-HIR subset is
    /// integer-arithmetic-only (`C-PY-INT-ARITH` territory). Other
    /// backends (rust / ruchy / lean) refuse `Expr::LitStr` via
    /// `Unsupported` arms naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    ///
    /// `LitStr` renders as a bareword in POSIX sh (no quoting). For
    /// args that contain whitespace / special chars, use
    /// `Expr::QuotedString` instead so the rendered shell carries
    /// the right quoting.
    LitStr(String),
    /// Quoted string literal carrying an explicit `QuotingStrategy`.
    /// PMAT-042 — the typed counterpart to `LitStr` when shell-level
    /// quoting matters (whitespace-containing args, vars-disabled
    /// args, etc.).
    ///
    /// The `content` is the *unescaped* string; the backend is
    /// responsible for emitting any escape sequences the chosen
    /// quoting style requires.
    QuotedString {
        content: String,
        quoting: QuotingStrategy,
    },
    /// Shell variable reference (`$NAME` or `${NAME}`). PMAT-045 /
    /// XPILE-BASHRS-MERGER-001 Layer B (Expr-side).
    ///
    /// The carried `String` is the variable name *without* the
    /// leading `$` and *without* the optional braces. bashrs-backend
    /// renders as `$NAME` by default. POSIX-legal name predicate
    /// (alphanumeric + underscore, no leading digit) is enforced
    /// at parse time, so a reachable `ShellVar` is always
    /// renderable as bareword.
    ///
    /// Same cross-domain disposition as the other Layer B Expr
    /// variants: produced only by `bashrs-frontend`, consumed only
    /// by `bashrs-backend`; rust/ruchy/lean refuse via
    /// `Unsupported(...)` naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    ShellVar(String),
    /// POSIX shell special parameter (`$1`..`$9`, `$0`, `$@`, `$*`,
    /// `$#`, `$?`, `$$`, `$!`, `$-`). PMAT-055.
    ///
    /// The carried `String` is the single-character name *without*
    /// the leading `$`. Distinct from `Expr::ShellVar` because
    /// special parameters are positional / runtime values set by
    /// the shell, not user-named variables. The bashrs-frontend
    /// parser tags them via this variant rather than ShellVar so
    /// future Lean refinement (Silver tier) can model them
    /// separately.
    ///
    /// bashrs-backend renders as `$<name>` (no braces — they're
    /// single-char so braces add nothing). Other backends refuse
    /// via `Unsupported(...)` naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    ShellSpecial(String),
    /// Shell command substitution (`$(cmd)`). PMAT-047 /
    /// XPILE-BASHRS-MERGER-001 Layer B (Expr-side, composes Stmt
    /// into Expr).
    ///
    /// The nested `Box<Stmt>` is typically a `Stmt::Cmd` (or future
    /// `Stmt::Pipeline`). bashrs-backend renders by recursing into
    /// the inner statement and wrapping with `$(...)`.
    ///
    /// IR-shape only at v0.1.0: bashrs-frontend's hand-rolled parser
    /// doesn't recognise `$(...)` syntax yet (it'd require a real
    /// tokenizer that respects nested grouping and quoting). The
    /// v0.2.0 source fold's bashrs parser produces this variant
    /// from real shell input. Until then this is a *capability
    /// declaration* — the IR can carry it, the backend can render
    /// it, only the parser side is missing.
    ///
    /// Same cross-domain disposition: bashrs-only; other backends
    /// refuse with `Unsupported(...)` naming `C-BASHRS-POSIX-IDEMPOTENCE`.
    CommandSubstitution(Box<Stmt>),
}

/// Per-arg shell quoting choice. Carried by `Expr::QuotedString` so
/// the bashrs-backend renders each arg in the strategy the source
/// indicated. v0.1.0's bashrs-frontend doesn't have a real
/// quoting-aware parser yet — it produces `Expr::LitStr` for every
/// arg, and `QuotingStrategy::None` is implicit there. The full
/// strategy set is wired so the v0.2.0 source fold's real bashrs
/// parser can plug in without IR churn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum QuotingStrategy {
    /// `'literal'` — single quotes. No variable expansion, no
    /// command substitution. Most idempotence-friendly form.
    Single,
    /// `"literal"` — double quotes. Variable expansion + command
    /// substitution still occur inside; only globbing/word-splitting
    /// are suppressed.
    Double,
    /// `\literal` — backslash-escape individual characters. Useful
    /// for short fragments where surrounding quotes would be
    /// awkward.
    Backslash,
}

/// PMAT-477 (R8): float arithmetic operators. Carried by
/// [`Expr::FloatBinOp`]. `Add`/`Sub`/`Mul`/`Div` emit plain infix (`f64`
/// saturates, no `checked_*`); `Div` is IEEE-754 true division.
/// PMAT-502br: `FloorDiv`/`Mod` emit Python-correct *floor* semantics
/// (`(a / b).floor()` and `a - b * (a / b).floor()`), which differ from
/// Rust's truncating `/` int-div and sign-of-dividend `%` — so they are
/// NOT plain infix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FloatOp {
    Add,
    Sub,
    Mul,
    Div,
    /// Python `a // b` over floats → `(a / b).floor()`.
    FloorDiv,
    /// Python `a % b` over floats → `a - b * (a / b).floor()`
    /// (result follows the divisor's sign, per Python).
    Mod,
    /// PMAT-502bt: Python `a ** b` with a float operand → `(a).powf(b)`
    /// (both operands are f64). Not infix.
    Pow,
}

/// PMAT-498 (Tranche 2): scalar numeric builtins carried by
/// [`Expr::NumBuiltin`]. `Abs` takes 1 arg; `Min`/`Max` take 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumBuiltinOp {
    /// `abs(x)` → `(x).abs()`
    Abs,
    /// `min(a, b)` → `(a).min(b)`
    Min,
    /// `max(a, b)` → `(a).max(b)`
    Max,
}

/// PMAT-495 (sprint): the iterator adapter for a [`Stmt::ForEachPair`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PairIterKind {
    /// `enumerate(iter, start)` — `first` = index (`i64`, offset by `start`,
    /// default 0), `second` = element of `iter`. Emits
    /// `.iter().cloned().enumerate().map(|(i,e)| (i as i64 + start, e))`
    /// (the `+ start` is omitted when `start == 0`). PMAT-502ca added `start`.
    Enumerate { start: i64 },
    /// `zip(iter, other)` — `first` = element of `iter`, `second` =
    /// element of `other`. Emits `.iter().cloned().zip(other.iter().cloned())`.
    Zip(Box<Expr>),
    /// `for first, second in <list of 2-tuples>` — e.g. `for k, v in
    /// d.items()`. PMAT-502y. The `iter` is already a `List(Tuple[A, B])`;
    /// each element is destructured into `(first, second)`. Emits
    /// `.iter().cloned()` (clone-based, non-consuming, like `Zip`).
    Pairs,
}

/// PMAT-492 (sprint): no-argument Python string transform methods,
/// carried by [`Expr::StrMethod`]. Each maps to a fixed Rust/Ruchy
/// receiver-method form; Lean/Shell refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrMethodOp {
    /// `.upper()` → `.to_uppercase()` (Str, 0 args)
    Upper,
    /// `.lower()` → `.to_lowercase()` (Str, 0 args)
    Lower,
    /// `.strip()` → `.trim().to_string()` (Str, 0 args)
    Strip,
    /// `.startswith(p)` → `.starts_with(&(p)[..])` (Bool, 1 arg). PMAT-493b.
    StartsWith,
    /// `.endswith(p)` → `.ends_with(&(p)[..])` (Bool, 1 arg). PMAT-493b.
    EndsWith,
    /// `.split(sep)` → `.split(&(sep)[..]).map(|s| s.to_string())
    /// .collect::<Vec<String>>()` (List(Str), 1 arg). PMAT-492c.
    Split,
    /// `.split()` (no arg) → `.split_whitespace().map(|s| s.to_string())
    /// .collect::<Vec<String>>()` (List(Str), 0 args). PMAT-502co. Python's
    /// no-arg split runs on any whitespace and drops empty fields, exactly
    /// like Rust's `split_whitespace`.
    SplitWhitespace,
    /// `.join(xs)` → `xs.join(&(sep)[..])` (Str, 1 list arg). PMAT-492d.
    /// NOTE the **receiver/arg inversion**: Python `sep.join(xs)` has the
    /// separator as receiver, but Rust's `[String]::join` has the list as
    /// receiver — so backends emit the arg as the Rust receiver.
    Join,
    /// `.replace(old, new)` → `.replace(&(old)[..], &(new)[..])`
    /// (Str, 2 args). PMAT-502b.
    Replace,
    /// `.lstrip()` → `.trim_start().to_string()` (Str, 0 args). PMAT-502l.
    LStrip,
    /// `.rstrip()` → `.trim_end().to_string()` (Str, 0 args). PMAT-502l.
    RStrip,
    /// `.find(sub)` → `.find(&(sub)[..]).map(|__i| __i as i64).unwrap_or(-1)`
    /// (**Int**, 1 arg) — byte index of the first match, or `-1`. PMAT-502l.
    /// (Python's `.find` is a *char* index; for ASCII — the v0.1.0 subset —
    /// byte and char indices coincide.)
    Find,
    /// `.count(sub)` → `.matches(&(sub)[..]).count() as i64` (**Int**, 1 arg)
    /// — count of non-overlapping occurrences. PMAT-502l.
    Count,
    /// `.index(sub)` → byte index of the first match (**Int**, 1 arg).
    /// PMAT-502bi. Like [`StrMethodOp::Find`] but **panics** when the
    /// substring is absent (matching Python's `ValueError`, vs `find`'s
    /// `-1`): `.find(&(sub)[..]).map(|__i| __i as i64).expect(…)`.
    /// (ASCII subset — byte index = char index.)
    StrIndex,
    /// `.isdigit()` → `(!(s).is_empty() && (s).chars().all(|c| c.is_ascii_digit()))`
    /// (**Bool**, 0 args). PMAT-502ag. Python returns `False` for the empty
    /// string, so the empty guard is required (a vacuous `.all()` is `true`).
    IsDigit,
    /// `.isalpha()` → `(!(s).is_empty() && (s).chars().all(|c| c.is_alphabetic()))`
    /// (**Bool**, 0 args). PMAT-502ag.
    IsAlpha,
    /// `.isspace()` → `(!(s).is_empty() && (s).chars().all(|c| c.is_whitespace()))`
    /// (**Bool**, 0 args). PMAT-502ag.
    IsSpace,
    /// `.capitalize()` → first char upper-cased, the rest lower-cased
    /// (**Str**, 0 args). PMAT-502ah. Emits a block that pops the first
    /// char (`to_uppercase`) and lower-cases the remainder; the empty
    /// string maps to `""` (matching Python).
    Capitalize,
    /// `.title()` → title-case: the first alphabetic char of each word is
    /// upper-cased, the rest lower-cased; any non-alphabetic char is a word
    /// boundary (**Str**, 0 args). PMAT-502aj. Emits a fold that tracks
    /// "previous char was alphabetic", matching Python's exact semantics
    /// (e.g. `"it's".title()` → `"It'S"`).
    Title,
    /// `.rjust(width)` → right-justify in a field of `width` (space-padded
    /// on the left) (**Str**, 1 int arg). PMAT-502aw. Emits
    /// `format!("{:>1$}", <recv>, (<width>) as usize)`. Rust's format width
    /// is a *minimum* — a longer string is returned unchanged, exactly
    /// matching Python (no truncation). A non-default fill char is deferred.
    RJust,
    /// `.ljust(width)` → left-justify (space-padded on the right)
    /// (**Str**, 1 int arg). PMAT-502aw. Emits
    /// `format!("{:<1$}", <recv>, (<width>) as usize)`.
    LJust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOp {
    /// Numeric negation: `-x`, I64 → I64. Note `i64::MIN`'s negation
    /// overflows; frontends should warn but emit anyway.
    Neg,
    /// Logical not: `not x`, Bool → Bool.
    Not,
}

/// PMAT-502g (Tranche 2): set-algebra operators carried by [`Expr::SetOp`].
/// Each maps to a `HashSet` method that returns an iterator of borrows,
/// materialized via `.cloned().collect()` into a new owned `HashSet`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetOp {
    /// `a | b` → `a.union(&b)`.
    Union,
    /// `a & b` → `a.intersection(&b)`.
    Intersection,
    /// `a - b` → `a.difference(&b)`.
    Difference,
    /// `a ^ b` → `a.symmetric_difference(&b)`.
    SymmetricDifference,
}

/// PMAT-502u (Tranche 2): list query methods carried by [`Expr::ListQuery`].
/// Both return `Int`; `Index` panics on a missing element (Python `ValueError`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListQueryOp {
    /// `xs.count(x)` → number of equal elements.
    Count,
    /// `xs.index(x)` → index of the first equal element (panics if absent).
    Index,
}

/// PMAT-502ap (Tranche 2): no-argument in-place list mutators carried by
/// [`Stmt::ListMutate`]. Each maps to the matching `Vec` method; all return
/// `None` in Python (so they only appear as expression statements).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListMutateOp {
    /// `xs.sort()` → `.sort()` (`Vec<i64>`) or `.sort_by(|a, b|
    /// a.partial_cmp(b).unwrap())` (`Vec<f64>`, see `Stmt::ListMutate.of_float`).
    Sort,
    /// `xs.reverse()` → `.reverse()` (element-type-agnostic).
    Reverse,
    /// `xs.clear()` → `.clear()` (element-type-agnostic).
    Clear,
}

/// PMAT-502z (Tranche 2): a `sorted(xs, key=lambda p: e)` sort key — the
/// lambda's single parameter name plus its (already-lowered) body. The body
/// is lowered with `param` left unbound (an `Ident`), so it works for bodies
/// that don't need the param's precise static type (arithmetic, `len(p)`,
/// most builtins); the backend emits `let <param> = __k.clone();` to bind it
/// by value at runtime. Str-method keys (`p.upper()`) are deferred.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortKey {
    pub param: String,
    pub body: Box<Expr>,
}

/// PMAT-502v (Tranche 2): dict view methods carried by [`Expr::DictView`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DictViewKind {
    /// `d.keys()` → `List(K)`.
    Keys,
    /// `d.values()` → `List(V)`.
    Values,
    /// `d.items()` → `List(Tuple[K, V])`. PMAT-502x. Emits
    /// `d.iter().map(|(__k, __v)| (__k.clone(), __v.clone())).collect::<Vec<_>>()`.
    Items,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    // Arithmetic — both operands `I64`, result `I64`.
    Add,
    Sub,
    Mul,
    /// Python `//`. Rust counterpart for signed: `a.div_euclid(b)` (matches
    /// Python's floor semantics). Plain `/` truncates toward zero in Rust,
    /// which diverges from Python for negative operands.
    FloorDiv,
    /// Python `%`. Rust counterpart: `a.rem_euclid(b)`. Same reason as FloorDiv.
    Mod,
    // Comparison — both operands `I64`, result `Bool`.
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    // Logical — both operands `Bool`, result `Bool`. Python's `and`/`or`
    // are short-circuiting; the Rust/Ruchy `&&`/`||` emissions match.
    And,
    Or,
    // Bitwise — both operands `I64`, result `I64`.
    // `&`, `|`, `^` lower to plain infix (no overflow risk on i64).
    // Shifts use `checked_shl` / `checked_shr` so an out-of-range shift
    // amount (>= 64) panics referencing the `C-PY-INT-ARITH` slow path,
    // matching the arithmetic ops' overflow contract. Note: bit-truncation
    // from a large left shift (e.g. `(1<<62) << 2`) is *not* detected by
    // `checked_shl`; that's also part of the bigint slow path.
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    /// Python `**`. Both operands `I64`, result `I64` for non-negative
    /// exponents that don't overflow. Lowers to `i64::checked_pow(u32)`
    /// in Rust/Ruchy; negative exponents panic (Python returns Float,
    /// which the v0.1.0 type system has no I64-compatible representation
    /// for — surfacing as a `C-PY-INT-ARITH` slow-path panic for now).
    Pow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiBoundary {
    pub from_lang: SourceLang,
    pub to_lang: SourceLang,
    pub symbol: String,
    pub signature: String,
}
