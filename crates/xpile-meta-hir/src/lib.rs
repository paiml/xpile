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
    /// PMAT-505a (classes epic, first cut): a Python `@dataclass` / field-only
    /// class → a Rust struct. `fields` are the annotated members in declaration
    /// order (`x: int` → `("x", Type::I64)`). Rust/Ruchy emit
    /// `#[derive(Clone, Debug, PartialEq)] pub struct Name { pub <f>: <ty>, … }`;
    /// Lean refuses (structure encoding deferred). This first cut emits the
    /// *definition* only — value construction (`Name(a, b)`) and field access
    /// (`obj.f`) are a follow-up sub-slice (they need a `Type::Struct` variant).
    Struct {
        name: String,
        fields: Vec<(String, Type)>,
        /// PMAT-506d (classes epic): instance methods → an `impl` block. Each is
        /// a [`Function`] whose first param is `self` (typed [`Type::Struct`] of
        /// this struct). Rust/Ruchy emit `impl Name { pub fn m(&self, …) … }`
        /// (the `self` param emits as `&self`); Lean refuses. First cut:
        /// read-only `&self` methods (self-mutating ones are rejected upstream).
        methods: Vec<Function>,
        /// PMAT-592 (classes epic): the class is `@dataclass(frozen=True)` — a
        /// frozen dataclass is *hashable* in Python, so it may be used as a
        /// dict key or set element. When this is set AND every field type is
        /// itself `Eq + Hash`-capable (`i64`/`bool`/`String`), the Rust/Ruchy
        /// codegen extends the derive list with `Eq, Hash` (a plain
        /// `#[derive(Clone, Debug, PartialEq)]` struct cannot be a `HashMap`
        /// key / `HashSet` element — E0277/E0599). Non-frozen dataclasses are
        /// unhashable in Python, so they keep the bare derive set. A float
        /// field disqualifies the struct (`f64` is neither `Eq` nor `Hash`),
        /// matching the codegen guard. `#[serde(default)]` for back-compat.
        #[serde(default)]
        frozen: bool,
        /// PMAT-648 (classes epic): the class is `@dataclass(order=True)` — Python
        /// generates `__lt__`/`__le__`/`__gt__`/`__ge__` comparing the fields as a
        /// tuple (definition order). The Rust/Ruchy codegen adds `PartialOrd` to
        /// the derive list (lexicographic by field order, matching Python's tuple
        /// comparison) so `Inst < Inst` etc. compile. `PartialOrd` is sound for
        /// any comparable field (incl. `f64`); full `Ord` (sorting instances) is a
        /// deferred follow-up (a float field can't derive `Ord`). Bare
        /// `@dataclass` / `order=False` keep the non-comparable derive set.
        /// `#[serde(default)]` for back-compat.
        #[serde(default)]
        order: bool,
    },
    /// PMAT-513 (Tranche 2): a Python `class C(Enum):` with `NAME = <int literal>`
    /// members → a Rust enum. `variants` are `(name, discriminant)` in declaration
    /// order. Rust/Ruchy emit
    /// `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum C { NAME, … }`;
    /// member access `C.NAME` → [`Expr::EnumVariant`] (`C::NAME`), and the
    /// compile-time-known `C.NAME.value` lowers directly to its discriminant
    /// literal. Enum-typed values reuse [`Type::Struct`] (an enum is just a named
    /// type at use sites). Lean refuses.
    Enum {
        name: String,
        variants: Vec<(String, i64)>,
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
        // PMAT-562: three-way zip — recurse into all three iterables + the body.
        Stmt::ForEachZip3 {
            iter1,
            iter2,
            iter3,
            body,
            ..
        } => {
            expr_has_int_arith(iter1)
                || expr_has_int_arith(iter2)
                || expr_has_int_arith(iter3)
                || body.iter().any(stmt_has_int_arith)
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
        // PMAT-502eg: list.remove(value) — recurse into the value expr.
        Stmt::ListRemoveValue { value, .. } => expr_has_int_arith(value),
        // PMAT-461: indexed assignment — recurse into both index and
        // value expressions (either may carry arithmetic).
        Stmt::IndexAssign { indices, value, .. } => {
            indices.iter().any(expr_has_int_arith) || expr_has_int_arith(value)
        }
        // PMAT-466 (v0.2.0 Track 1.C): dict keyed assignment — recurse
        // into both key and value expressions.
        Stmt::DictSet { key, value, .. } => expr_has_int_arith(key) || expr_has_int_arith(value),
        // PMAT-533: subscript-receiver append — recurse into index + elem.
        Stmt::IndexAppend { index, elem, .. } => {
            expr_has_int_arith(index) || expr_has_int_arith(elem)
        }
        // PMAT-727: setdefault-append — recurse into key, default, and elem.
        Stmt::DictSetdefaultAppend {
            key, default, elem, ..
        } => expr_has_int_arith(key) || expr_has_int_arith(default) || expr_has_int_arith(elem),
        // PMAT-730: nested subscript assign — recurse into each step index + value.
        Stmt::NestedSubscriptAssign { steps, value, .. } => {
            steps.iter().any(|(i, _)| expr_has_int_arith(i)) || expr_has_int_arith(value)
        }
        // PMAT-506c: field assignment — the assigned value may carry int arith.
        Stmt::FieldAssign { value, .. } => expr_has_int_arith(value),
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
        // PMAT-502dt: a block-expr recurses into its statements + trailing.
        Expr::Block(b) => {
            b.stmts.iter().any(stmt_has_int_arith) || expr_has_int_arith(&b.trailing_return)
        }
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
        // PMAT-502cv: hex/oct/bin — recurse into the value expr.
        Expr::IntRadixStr { value, .. } => expr_has_int_arith(value),
        Expr::IntFromStrRadix { value, .. } => expr_has_int_arith(value),
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
        Expr::Sum { list, start, .. } => {
            expr_has_int_arith(list) || start.as_ref().is_some_and(|s| expr_has_int_arith(s))
        }
        // PMAT-502j: all(xs)/any(xs) — recurse into the bool list.
        Expr::BoolReduce { list, .. } => expr_has_int_arith(list),
        // PMAT-502m: int(x)/float(x) — recurse into the converted value.
        Expr::NumCast { value, .. } => expr_has_int_arith(value),
        // PMAT-502ad: str(x) — recurse into the converted value.
        // PMAT-582: repr(str) — recurse into the value (a string, no int arith).
        Expr::ToStr { value, .. } | Expr::ReprStr { value } => expr_has_int_arith(value),
        // PMAT-502ak: round(x) — recurse into the rounded value.
        Expr::RoundToInt { value } => expr_has_int_arith(value),
        // PMAT-502al: round(x, n) — recurse into the value and ndigits.
        Expr::RoundToDigits { value, ndigits } => {
            expr_has_int_arith(value) || expr_has_int_arith(ndigits)
        }
        // PMAT-612: round(int, n) — recurse into the value and ndigits. The
        // banker's-rounding arithmetic itself is done in `i128` (guarded), so
        // the node does not need bigint treatment beyond its operands.
        Expr::RoundIntToDigits { value, ndigits } => {
            expr_has_int_arith(value) || expr_has_int_arith(ndigits)
        }
        // PMAT-502k: seq * n — recurse into both the sequence and count.
        Expr::Repeat { seq, n, .. } => expr_has_int_arith(seq) || expr_has_int_arith(n),
        // PMAT-502c: sorted — recurse into the list expression.
        Expr::Sorted { list, key, .. } => {
            expr_has_int_arith(list) || key.as_ref().is_some_and(|k| expr_has_int_arith(&k.body))
        }
        Expr::Reversed { list } => expr_has_int_arith(list),
        // PMAT-549/550: gcd/lcm carry int operands (the `%`/`*` are internal).
        Expr::Gcd { a, b } | Expr::Lcm { a, b } => expr_has_int_arith(a) || expr_has_int_arith(b),
        // PMAT-551/552: factorial/isqrt carry an int operand (loop arith internal).
        Expr::Factorial { n } | Expr::Isqrt { n } => expr_has_int_arith(n),
        // PMAT-553/554: comb/perm carry int operands (the loop arith is internal).
        Expr::Comb { n, k } | Expr::Perm { n, k } => expr_has_int_arith(n) || expr_has_int_arith(k),
        Expr::PowMod { base, exp, modulus } => {
            expr_has_int_arith(base) || expr_has_int_arith(exp) || expr_has_int_arith(modulus)
        }
        // PMAT-502cj: list(range(...)) — recurse into the bound exprs.
        Expr::RangeList { start, stop, .. } => {
            expr_has_int_arith(start) || expr_has_int_arith(stop)
        }
        // PMAT-502cw: set(xs) — recurse into the list expr.
        Expr::SetFromList { list } => expr_has_int_arith(list),
        Expr::SetToList { set } => expr_has_int_arith(set),
        Expr::DictFromPairs { pairs } => expr_has_int_arith(pairs),
        Expr::DictMerge { entries } => entries
            .iter()
            .any(|(k, v)| k.as_ref().is_some_and(expr_has_int_arith) || expr_has_int_arith(v)),
        // PMAT-502ab: filter — recurse into the list and predicate body.
        Expr::Filter { list, lambda } => {
            expr_has_int_arith(list) || expr_has_int_arith(&lambda.body)
        }
        // PMAT-502ac: map — recurse into the list and transform body.
        Expr::Map { list, lambda } => expr_has_int_arith(list) || expr_has_int_arith(&lambda.body),
        // PMAT-502ai: enumerate/zip — recurse into the source list(s).
        Expr::Enumerate { list, .. } => expr_has_int_arith(list),
        Expr::Zip { left, right } => expr_has_int_arith(left) || expr_has_int_arith(right),
        Expr::ListMinMax {
            list, key, default, ..
        } => {
            expr_has_int_arith(list)
                || key.as_ref().is_some_and(|k| expr_has_int_arith(&k.body))
                || default.as_ref().is_some_and(|d| expr_has_int_arith(d))
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
        // PMAT-502ep: set predicates — recurse into both operands.
        Expr::SetPred { lhs, rhs, .. } => expr_has_int_arith(lhs) || expr_has_int_arith(rhs),
        // PMAT-502eq: shallow copy — recurse into the cloned value.
        Expr::Clone(inner) => expr_has_int_arith(inner),
        // PMAT-502ew: Option wrapper — recurse into the `Some(e)` payload.
        Expr::OptionExpr(inner) => inner.as_deref().is_some_and(expr_has_int_arith),
        // PMAT-721: Optional truthiness — recurse into the value (the `__v != 0`
        // body is a comparison, never overflowing int arithmetic).
        Expr::OptionTruthy { value, .. } => expr_has_int_arith(value),
        // PMAT-724: `x or default` over Optional — recurse into value + default
        // (the truthiness body is a non-overflowing comparison).
        Expr::OptionOrDefault { value, default, .. } => {
            expr_has_int_arith(value) || expr_has_int_arith(default)
        }
        // PMAT-502ex: `is None` test — recurse into the tested value.
        Expr::IsNone { value, .. } => expr_has_int_arith(value),
        // PMAT-502ez: unwrap recurses into the inner operand.
        Expr::OptionUnwrap(inner) => expr_has_int_arith(inner),
        // PMAT-503b: try/except recurses into both the body and the handler.
        Expr::TryCatch { body, handler } => expr_has_int_arith(body) || expr_has_int_arith(handler),
        // PMAT-506b: a struct literal's field values may contain int arith;
        // a field read does not by itself.
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, v)| expr_has_int_arith(v)),
        Expr::FieldAccess { obj, .. } => expr_has_int_arith(obj),
        Expr::MethodCall { obj, args, .. } => {
            expr_has_int_arith(obj) || args.iter().any(expr_has_int_arith)
        }
        // PMAT-513: an enum member access is a constant — no int arithmetic.
        Expr::EnumVariant { .. } => false,
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
        // PMAT-502ey: 1-arg dict get — recurse into dict + key.
        Expr::DictGetOpt { dict, key } => expr_has_int_arith(dict) || expr_has_int_arith(key),
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// `names`. Rust/Ruchy emit `let (a, b, ...) = <value>;`, marking
    /// `names[i]` `mut` when `mutable[i]` (PMAT-547: a later
    /// reassignment/augment of an unpacked name); Lean refuses. Nested /
    /// starred / subscript patterns are not supported at first cut (all
    /// targets must be plain names). `mutable` is parallel to `names`.
    LetTuple {
        names: Vec<String>,
        mutable: Vec<bool>,
        value: Expr,
    },
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
        /// The index path, base→leaf. A single index is `xs[i] = v`; a
        /// multi-element path is nested list indexing (`grid[i][j] = v`,
        /// PMAT-502dy) — every index is `usize`-coerced (all-list nesting).
        indices: Vec<Expr>,
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
    /// PMAT-533: in-place `append` on a **subscript receiver** — Python
    /// `xs[i].append(e)` (list-of-list) or `d[k].append(e)` (dict-of-list).
    /// The bare-statement `<name>.append(e)` form is [`Stmt::ListAppend`]; this
    /// is the indexed-receiver companion (the receiver is itself a list reached
    /// through one subscript).
    ///
    /// Constraints:
    ///   - `base` is a bound name typing as `list[list[T]]` (`base_is_dict =
    ///     false`) or `dict[K, list[T]]` (`base_is_dict = true`).
    ///   - The base is marked mutable (the pre-walk recognises a subscript
    ///     receiver too).
    ///   - For a list base the index `usize`-coerces (matching `IndexAssign`);
    ///     for a dict base the value is reached via `get_mut(&k).unwrap()`
    ///     (KeyError-on-absent parity with Python).
    ///
    /// Backends:
    ///   * Rust / Ruchy: `base[(index) as usize].push(elem);` (list) or
    ///     `base.get_mut(&(index)).unwrap().push(elem);` (dict).
    ///   * Lean / Shell: refuse (in-place mutation, same gap as `ListAppend`).
    IndexAppend {
        base: String,
        index: Expr,
        elem: Expr,
        base_is_dict: bool,
    },
    /// PMAT-727 (HUNT-V10 V10-8): the grouping idiom `d.setdefault(k,
    /// <default>).append(elem)` — get-or-insert the key's list then append. Unlike
    /// [`Stmt::IndexAppend`] (`d[k].append` panics KeyError on an absent key), this
    /// CREATES the entry when absent. Rust/Ruchy emit
    /// `d.entry(<key>).or_insert_with(|| <default>).push(<elem>);`; Lean refuses
    /// (in-place dict mutation). `default` is the setdefault default (typically an
    /// empty list, threaded to the dict's value type).
    DictSetdefaultAppend {
        dict: String,
        key: Expr,
        default: Expr,
        elem: Expr,
    },
    /// PMAT-730 (HUNT-V10 V10-7): a nested subscript assignment where at least one
    /// level is a DICT — `d[a][b] = v` over `dict[K, dict[K2, V]]`, or a mixed
    /// `dm[k][i] = v` over `dict[K, list[T]]`. Each `(index, is_dict)` step is the
    /// container kind at that level, base→leaf. Rust/Ruchy navigate the
    /// intermediate levels with `get_mut(&k).unwrap()` (dict) / `[i as usize]`
    /// (list) and assign at the leaf with `.insert(k, v)` (dict) / `[i as usize] =
    /// v` (list). Lean refuses (in-place nested mutation). All-LIST nesting stays
    /// on [`Stmt::IndexAssign`]; this carries the per-level kind a dict level needs.
    NestedSubscriptAssign {
        base: String,
        /// `(index, is_dict)` per level, base→leaf; `len >= 2`.
        steps: Vec<(Expr, bool)>,
        value: Expr,
    },
    /// PMAT-506c (classes epic): struct field assignment — Python `obj.field =
    /// value`. Rust/Ruchy emit `(<obj>).<field> = <value>;` (the `obj` binding
    /// must be `mut`, ensured by the mutability pre-walk); Lean refuses. First
    /// cut: `obj` is a plain bound name (a struct local/param).
    FieldAssign {
        obj: String,
        field: String,
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
    /// tail right; the receiver is marked mutable. Rust/Ruchy emit a
    /// CPython-clamping block (PMAT-590) that normalizes a negative
    /// `index` to `len + index` (clamped to `0`) and caps `index > len`
    /// at `len`, matching `list.insert` (listobject.c `ins1`) rather than
    /// panicking like a bare `Vec::insert`. Lean refuses (in-place
    /// mutation, same gap as `ListAppend`).
    ListInsert {
        list_name: String,
        index: Expr,
        elem: Expr,
    },
    /// Remove-by-value — Python `xs.remove(x)`. PMAT-502eg (Tranche 2).
    /// Removes the *first* element equal to `value`, shifting the tail
    /// left; raises `ValueError` if absent. The receiver is marked
    /// mutable. Rust/Ruchy emit a position-find + `Vec::remove`, panicking
    /// (≈ Python `ValueError`) when the value isn't present:
    /// `{ let __v = <value>; let __p = <list>.iter().position(|__e| *__e == __v).expect("…"); <list>.remove(__p); }`.
    /// Distinct from set `.remove` ([`Stmt::SetRemove`], which removes by
    /// key); the frontend's receiver type disambiguates. Lean refuses
    /// (in-place mutation, same gap as `ListAppend`).
    ListRemoveValue { list_name: String, value: Expr },
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
    /// PMAT-562: three-way `zip` for-loop — Python `for a, b, c in zip(x, y, z)`.
    /// Rust/Ruchy emit `for ((a, b), c) in x.iter().cloned().zip(y.iter()
    /// .cloned()).zip(z.iter().cloned()) { body }` (left-nested zip + nested
    /// destructure; stops at the shortest iterable, like Python `zip`). A
    /// separate variant from [`Stmt::ForEachPair`] for the third binding. Lean
    /// refuses.
    ForEachZip3 {
        first: String,
        second: String,
        third: String,
        iter1: Expr,
        iter2: Expr,
        iter3: Expr,
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
    /// PMAT-502ew: Python `Optional[T]` (a value that may be `None`).
    /// Rust/Ruchy emit `Option<T>`; Lean emits `Option T`. First cut
    /// (PMAT-502ew) supports it only as a function *return* type, with the
    /// returned values wrapped via [`Expr::OptionExpr`] (`Some(x)` / `None`);
    /// `Optional` parameters / locals and `is None` flow-narrowing are a
    /// deferred follow-up.
    Optional(Box<Type>),
    /// PMAT-506b (classes epic): a named struct type — a value of a Python
    /// `@dataclass` / class lowered via [`Item::Struct`]. Carries the struct's
    /// name; Rust/Ruchy emit the bare name (`Point`); Lean refuses (struct
    /// values deferred). Produced for struct-typed params/returns/locals,
    /// [`Expr::StructLit`] construction, and [`Expr::FieldAccess`] receivers.
    Struct(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// The unit value `()` — the trailing "return" of a void function
    /// (Python `-> None`). PMAT-502bl (Tranche 2). Rust/Ruchy emit `()`;
    /// Lean refuses. Types as [`Type::Unit`].
    Unit,
    /// A block expression — zero or more statements followed by a trailing
    /// value. PMAT-502dt (Tranche 2). Rust/Ruchy emit `{ <stmts> <trailing> }`;
    /// types as the trailing expression's type. The first producer is the
    /// multi-statement nested-function body (`ClosureLet`'s body). A reusable
    /// primitive for future expression-position comprehensions. Lean refuses.
    Block(Box<Block>),
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
    /// PMAT-502ey: 1-arg `d.get(k)` (no default) — Python returns the value or
    /// `None`. Rust/Ruchy emit `(<dict>).get(&(<key>)).cloned()` → `Option<V>`;
    /// types as [`Type::Optional`] of the value type. Lean refuses (Optional
    /// deferred). The 2-arg `d.get(k, default)` form stays [`DictGetOr`].
    DictGetOpt { dict: Box<Expr>, key: Box<Expr> },
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
    /// PMAT-502ep: set predicate — Python `a <= b` / `a < b` / `a >= b` /
    /// `a > b` over two sets (subset / proper-subset / superset /
    /// proper-superset) and the method forms `a.issubset(b)` /
    /// `a.issuperset(b)` / `a.isdisjoint(b)`. Yields `bool`. Rust/Ruchy emit a
    /// temp-bound block over `HashSet::is_subset`/`is_superset`/`is_disjoint`
    /// (proper variants add `&& __l != __r`). Lean refuses.
    SetPred {
        lhs: Box<Expr>,
        op: SetPredOp,
        rhs: Box<Expr>,
    },
    /// PMAT-502eq: a shallow copy — Python `xs.copy()` / `d.copy()` /
    /// `s.copy()` over a list / dict / set. Yields a **new** owned collection of
    /// the same type. Rust/Ruchy emit `(<inner>).clone()`; Lean emits the inner
    /// expression directly (Lean values are immutable, so a copy is identity).
    Clone(Box<Expr>),
    /// PMAT-502ew: an `Option` value — `None` (`OptionExpr(None)`) or
    /// `Some(e)` (`OptionExpr(Some(e))`). Produced when wrapping the returns of
    /// an `Optional[T]`-returning function: `return None` → `None`, `return x`
    /// → `Some(x)`. Rust/Ruchy emit `None` / `Some(<e>)`; Lean `none` / `some
    /// (<e>)`. Types as [`Type::Optional`].
    OptionExpr(Option<Box<Expr>>),
    /// PMAT-721 (HUNT-V9 V9-18): Python truthiness of an `Optional[T]` value in a
    /// boolean context (`if x:`, `while x:`, `assert x`, a ternary/comprehension
    /// condition). None is falsy; `Some(v)` is truthy iff `v` is truthy. Lowers to
    /// `(<value>)[.as_ref()].is_some_and(|__v| <body>)`, where `body` is the inner
    /// type's truthiness over the bound `__v` (built by `truthy_condition`).
    /// `by_ref` is set for a non-`Copy` inner (`str`/`list`/`dict`/`set`) so the
    /// value is not consumed (`.as_ref()` first); a `Copy` inner (`int`/`float`/
    /// `bool`) takes the value directly. Yields `bool`.
    OptionTruthy {
        value: Box<Expr>,
        by_ref: bool,
        body: Box<Expr>,
    },
    /// PMAT-724 (HUNT-V9 V9-19): Python `x or default` where `x` is `Optional[T]`
    /// and `default` is `T` — returns the inner value when `x` is truthy, else
    /// `default`. Lowers to `(<value>).filter(|<param>| <body>).unwrap_or_else(||
    /// <default>)` where `param` is `&__v` for a `Copy` inner (the `body` is the
    /// value form) and `__v` for a non-`Copy` inner (the `body` is the
    /// `&`-borrowing `Len` form). `filter` always hands the predicate a `&T`, so
    /// `by_ref` selects the closure-param pattern (not an `.as_ref()`).
    /// `unwrap_or_else` keeps Python's short-circuit (the default is lazy). Yields
    /// `T`.
    OptionOrDefault {
        value: Box<Expr>,
        by_ref: bool,
        body: Box<Expr>,
        default: Box<Expr>,
    },
    /// PMAT-502ex: a `None` test over an `Optional` value — Python `x is None`
    /// (`negated == false`) / `x is not None` (`negated == true`). Yields
    /// `bool`. Rust/Ruchy emit `(<value>).is_none()` / `.is_some()`; Lean
    /// refuses (Optional deferred there).
    IsNone { value: Box<Expr>, negated: bool },
    /// PMAT-502ez (Optional epic cut 4): the unwrapped value of an `Optional`
    /// that flow-narrowing has proven to be `Some`. Produced when a name guarded
    /// by a preceding `if x is None: return …` (a provably-exiting None-guard) is
    /// later read in value position — the guard guarantees `Some`, so the read
    /// lowers to `(<inner>).unwrap()` : `T`. Rust/Ruchy emit `(<inner>).unwrap()`;
    /// Lean refuses (Optional deferred). Types as the inner type of the operand's
    /// [`Type::Optional`].
    OptionUnwrap(Box<Expr>),
    /// PMAT-503b (exceptions epic): a value-producing `try`/`except` —
    /// Python `try: return <body> except [E]: return <handler>`. xpile models
    /// Python exceptions as Rust panics (KeyError → `.expect`, ZeroDivisionError,
    /// index-out-of-bounds, …), so the `except` catches those panics: Rust/Ruchy
    /// emit a `std::panic::catch_unwind(AssertUnwindSafe(|| <body>))` match —
    /// `Ok(v) => v`, `Err(_) => <handler>`. Lean refuses (no panic model). Types
    /// as the `body` type (the `handler` must produce the same type). First cut:
    /// catch-all (the exception type, if named, is not matched — Rust panics are
    /// untyped) with no bound exception object, no `else`/`finally`.
    TryCatch { body: Box<Expr>, handler: Box<Expr> },
    /// PMAT-506b (classes epic): struct construction — Python `Name(a, b)` over
    /// a `@dataclass`/class. `fields` are `(field_name, value)` in declaration
    /// order. Rust/Ruchy emit `Name { f0: v0, f1: v1, … }`; Lean refuses. Types
    /// as [`Type::Struct`]`(name)`.
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    /// PMAT-506b (classes epic): struct field read — Python `obj.field`. Rust/
    /// Ruchy emit `(<obj>).<field>`; Lean refuses. Types as the field's type
    /// (looked up in the struct registry at lowering time).
    FieldAccess { obj: Box<Expr>, field: String },
    /// PMAT-506d (classes epic): struct method call — Python `obj.method(args)`.
    /// Rust/Ruchy emit `(<obj>).<method>(<args>)`; Lean refuses. Types as the
    /// method's return type (from the struct-method registry).
    MethodCall {
        obj: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    /// PMAT-513 (Tranche 2): an enum member access — Python `C.NAME` where `C` is
    /// an `Enum` class. Rust/Ruchy emit `C::NAME`; Lean refuses. Types as
    /// [`Type::Struct`]`(enum_name)` (an enum is a named type at use sites).
    EnumVariant { enum_name: String, variant: String },
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
    /// `hex(n)` / `oct(n)` / `bin(n)` — the radix string of an int (→ `str`).
    /// PMAT-502cv (Tranche 2). Python prefixes `0x`/`0o`/`0b` and puts the
    /// sign first for negatives (`hex(-255)` = `"-0xff"`). Rust/Ruchy emit
    /// `{ let __n = (<v>); let __m = __n.unsigned_abs(); let __sign = if
    /// __n < 0 { "-" } else { "" }; format!("{}<prefix>{:<spec>}", __sign,
    /// __m) }` (`__m` is the magnitude so i64::MIN is safe). Lean refuses.
    /// PMAT-502dp: `prefixed` controls the `0x`/`0o`/`0b` prefix (`true` for
    /// the `hex`/`oct`/`bin` builtins; `false` for printf `%x`/`%X`/`%o`, which
    /// emit bare digits). `upper` selects upper-case hex (`%X`).
    IntRadixStr {
        value: Box<Expr>,
        radix: Radix,
        prefixed: bool,
        upper: bool,
    },
    /// `int(s, base)` — parse a string in the given radix (→ `int`).
    /// PMAT-502da (Tranche 2); the str→int reverse of [`Expr::IntRadixStr`].
    /// `radix` is a literal `2..=36`. Rust/Ruchy emit
    /// `i64::from_str_radix((<value>).trim(), <radix>).expect("…")` — a parse
    /// failure (or an out-of-range digit) panics, ≈ Python's `ValueError`.
    /// (A non-literal / out-of-range base is rejected in the frontend; the
    /// auto-detect `int(s, 0)` form is deferred.) Lean refuses.
    IntFromStrRadix { value: Box<Expr>, radix: u32 },
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
    ///
    /// PMAT-579: `of_float` (set by the frontend from the first argument's type)
    /// is consulted only for [`NumBuiltinOp::Abs`]: an `i64` `abs` emits
    /// `.checked_abs().expect(…)` so `abs(i64::MIN)` PANICS (per C-PY-INT-ARITH;
    /// `i64::MIN.abs()` would otherwise wrap to `i64::MIN` silently), while an
    /// `f64` `abs` keeps `.abs()` (no overflow). `min`/`max` and the float math
    /// builtins ignore it.
    NumBuiltin {
        op: NumBuiltinOp,
        args: Vec<Expr>,
        #[serde(default)]
        of_float: bool,
    },
    /// `sum(xs)` / `sum(xs, start)` over a numeric list — Python builtin.
    /// PMAT-498b (Tranche 2); 2-arg `start` added PMAT-502cx. Rust/Ruchy
    /// emit `<list>.iter().sum::<T>()` with the turbofish `T` selected by
    /// `of_float` (the frontend sets it from the element type — `i64` for
    /// `list[int]`, `f64` for `list[float]`). When `start` is present it is
    /// prepended: `(<start>) + <list>.iter().sum::<T>()` (Python's
    /// `sum(xs, start) == start + sum(xs)`); the frontend requires `start`
    /// to match the element type (`int` start for an int list, `float`
    /// start for a float list) so no cast is emitted. Result types as the
    /// element type. Lean refuses.
    Sum {
        list: Box<Expr>,
        of_float: bool,
        start: Option<Box<Expr>>,
    },
    /// `all(xs)` / `any(xs)` over a `list[bool]` — Python builtins.
    /// PMAT-502j (Tranche 2). Rust/Ruchy emit
    /// `<list>.iter().all(|&__b| __b)` (or `.any(…)`); result types as
    /// `Bool`. Like Python, `all([])` is `true` and `any([])` is `false`
    /// (the iterator-adaptor identities). Lean refuses.
    ///
    /// PMAT-689: `short_circuit` is set when the source was a GENERATOR
    /// expression (`any(P(x) for x in xs)`), which Python evaluates LAZILY — the
    /// backend then fuses the inner `Map`'s predicate into the `any`/`all` closure
    /// (`xs.iter().cloned().any(|x| P(x))`) so a not-yet-needed element is never
    /// evaluated (matching Python's short-circuit; the prior eager
    /// `.map(P).collect().iter().any(..)` panicked on e.g. a div-by-zero element
    /// Python never reaches). A LIST comprehension (`any([P(x) for x in xs])`) is
    /// eager in Python, so it stays `false` (no fusion) and keeps the eager form.
    BoolReduce {
        list: Box<Expr>,
        is_all: bool,
        short_circuit: bool,
    },
    /// Sequence repetition — Python `seq * n` / `n * seq` where `seq` is a
    /// `Str` or `List` and `n` an `Int`. PMAT-502k (Tranche 2). The `.max(0)`
    /// clamps a negative count to the empty sequence, matching Python
    /// (`"x" * -1 == ""`). Result types as `seq`. Lean refuses.
    ///
    /// PMAT-569: `of_str` selects the emit. A **str** repeat uses
    /// `(<seq>).repeat(...)` (`String::repeat`, no `Copy` bound). A **list**
    /// repeat must NOT use slice `repeat` — that requires `T: Copy`, so
    /// `[[0]] * n` (a `Vec<Vec<_>>`) fails to compile (E0277). Instead a list
    /// clones its elements: `{ let __rep = (<seq>); (0..k).flat_map(|_|
    /// __rep.iter().cloned()).collect::<Vec<_>>() }`, which works for any
    /// `Clone` element. (Under xpile's value semantics the repeated rows are
    /// independent, unlike CPython's aliasing — consistent with how every other
    /// list copy behaves here.)
    Repeat {
        seq: Box<Expr>,
        n: Box<Expr>,
        of_str: bool,
    },
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
        /// PMAT-586: set when the source operand is a `float` being cast to an
        /// `int` (`int(float_x)`). Python raises `OverflowError` for `int(inf)`
        /// and `ValueError` for `int(nan)`, but Rust's `as i64` saturates
        /// (`inf` → `i64::MAX`) / zeroes (`nan` → 0) silently, so the int-cast
        /// codegen guards a non-finite source and panics. Irrelevant (false)
        /// for `int(int)`, `float(_)`, and the `from_str` parse paths.
        #[serde(default)]
        from_float: bool,
    },
    /// Python `str(x)` over an **int** or **float** `x` → its string form.
    /// PMAT-502ad (int); `of_float` is PMAT-502af. For int, Rust/Ruchy emit
    /// `format!("{}", <value>)`. For float they emit a block that matches
    /// Python's formatting (`is_nan()` → `"nan"`; finite whole numbers get a
    /// `".0"` suffix; otherwise `format!("{}", …)`), since Rust's bare
    /// `format!` prints e.g. `2.0` as `"2"`. Result types as `Str`.
    /// (`str(bool)` desugars to an `IfExpr`, PMAT-502ae.) Lean refuses.
    ToStr { value: Box<Expr>, of_float: bool },
    /// PMAT-582: Python `repr(s)` over a **string** `s` → its quoted
    /// representation. `repr` of an int/float/bool equals `str` of it, so the
    /// frontend lowers those through [`Expr::ToStr`] / the `str(bool)` desugar;
    /// only the string case (which adds quotes + escapes) needs this node.
    /// Rust/Ruchy emit an inline block that picks the quote like CPython
    /// (single quotes, switching to double if the string contains a `'` but no
    /// `"`) and escapes `\`, the quote, `\n`, `\r`, `\t`. (Other non-printables
    /// are emitted verbatim — full `\xNN`/`\uNNNN` escaping is deferred.)
    /// Result types as `Str`. Lean refuses. Container `repr` + f-string `{x!r}`
    /// are separate, deferred slices.
    ReprStr { value: Box<Expr> },
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
    /// `round(x, n)` over an **int** `x` and **int** `n` → an **int**.
    /// PMAT-612 (Tranche 2). Python `round(int, n)` is the identity for
    /// `n >= 0` (an int has no fractional part), and rounds to the nearest
    /// multiple of `10^(-n)` using round-half-to-**even** (banker's rounding)
    /// for `n < 0` (`round(12350, -2) == 12400`, `round(12250, -2) == 12200`).
    /// Rust/Ruchy emit a block doing the arithmetic in `i128` (so the scale
    /// `10^(-n)` and the products don't overflow), failing loud if the rounded
    /// result leaves `i64` range. Lean refuses.
    RoundIntToDigits {
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
    ///
    /// PMAT-578: `of_float` (set by the frontend from the list element type)
    /// selects the **keyless** comparator — `Vec<f64>` has no `Ord`, so a float
    /// list sorts via `sort_by(|a, b| a.partial_cmp(b).unwrap())` (descending:
    /// `b.partial_cmp(a)`), mirroring [`ListMutateOp::Sort`]; an `i64` list keeps
    /// `.sort()`. (NaN panics, matching Python's undefined NaN-sort behaviour.)
    /// Only consulted when `key` is `None`; a float-returning `key` is a
    /// separate, deferred case.
    Sorted {
        list: Box<Expr>,
        reverse: bool,
        key: Option<SortKey>,
        #[serde(default)]
        of_float: bool,
    },
    /// `list(reversed(xs))` / `reversed(xs)` over a list — Python builtin
    /// returning a **new** reversed list (the input is not mutated).
    /// PMAT-502d (Tranche 2). Rust/Ruchy emit
    /// `{ let mut __v = <list>.clone(); __v.reverse(); __v }`; result types
    /// as the list's type. Lean refuses. (Python's `reversed` yields a lazy
    /// iterator, but the supported subset materializes it as a `Vec`.)
    Reversed { list: Box<Expr> },
    /// PMAT-549: `math.gcd(a, b)` — greatest common divisor of two ints (**Int**).
    /// Rust/Ruchy emit an inline Euclidean-algorithm block over the operands'
    /// absolute values: `{ let mut __a = (a).abs(); let mut __b = (b).abs();
    /// while __b != 0 { let __t = __b; __b = __a % __b; __a = __t; } __a }`.
    /// Always non-negative; `gcd(0, 0) == 0` (matching Python). Lean refuses.
    Gcd { a: Box<Expr>, b: Box<Expr> },
    /// PMAT-550: `math.lcm(a, b)` — least common multiple of two ints (**Int**).
    /// Rust/Ruchy emit `{ let __la=(a).abs(); let __lb=(b).abs(); if __la==0 ||
    /// __lb==0 { 0 } else { <Euclid gcd of __la,__lb → __ga>; (__la / __ga) *
    /// __lb } }` — divide before multiply to limit overflow. `lcm(0, x) == 0`
    /// (matching Python); always non-negative. Lean refuses.
    Lcm { a: Box<Expr>, b: Box<Expr> },
    /// PMAT-551: `math.factorial(n)` — n! of a non-negative int (**Int**).
    /// Rust/Ruchy emit `{ let __nf = (n); if __nf < 0 { panic!(…ValueError…) }
    /// let mut __f = 1i64; let mut __i = 2i64; while __i <= __nf { __f = __f
    /// .checked_mul(__i).expect(…overflow…); __i += 1; } __f }` — `0! == 1! ==
    /// 1`; overflow panics under the i64 int-arith contract; a negative `n`
    /// panics (Python `ValueError`). Lean refuses.
    Factorial { n: Box<Expr> },
    /// PMAT-552: `math.isqrt(n)` — exact integer square root `⌊√n⌋` of a
    /// non-negative int (**Int**). Rust/Ruchy emit an inline integer-Newton
    /// block (no float, so exact for every `i64`); a negative `n` panics (Python
    /// `ValueError`); `isqrt(0) == 0`. Lean refuses.
    Isqrt { n: Box<Expr> },
    /// PMAT-553: `math.comb(n, k)` — binomial coefficient "n choose k" (**Int**).
    /// Rust/Ruchy emit an inline incremental-product block (`C(n,i+1) =
    /// C(n,i)*(n-i)/(i+1)`, iterating `min(k, n-k)` times so each partial stays a
    /// true binomial). `0` when `k > n` (with both non-negative); a negative `n`
    /// or `k` panics (Python `ValueError`). Like all i64 arithmetic the running
    /// `checked_mul` panics on overflow (a result whose intermediate exceeds i64).
    /// Lean refuses.
    Comb { n: Box<Expr>, k: Box<Expr> },
    /// PMAT-554: `math.perm(n, k)` — number of `k`-permutations of `n`,
    /// `P(n, k) = n! / (n - k)!` (**Int**). Rust/Ruchy emit an inline product
    /// block (`∏_{i=0}^{k-1} (n - i)`, i.e. `k` descending factors from `n`).
    /// `0` when `k > n` (with both non-negative); a negative `n` or `k` panics
    /// (Python `ValueError`). The running `checked_mul` panics on overflow. The
    /// one-arg form `math.perm(n)` lowers to [`Expr::Factorial`] at the frontend
    /// (`perm(n) == n!`), so only the two-arg form reaches here. Lean refuses.
    Perm { n: Box<Expr>, k: Box<Expr> },
    /// PMAT-571: 3-arg `pow(base, exp, modulus)` — modular exponentiation
    /// `base**exp mod modulus` (**Int**). Rust/Ruchy emit an inline
    /// square-and-multiply block that reduces modulo `modulus` at each step
    /// (so it never overflows for a non-negative result), using `i128`
    /// intermediates for the products. The result is normalised to `[0, m)`
    /// for a positive modulus (matching Python). A zero modulus or a negative
    /// exponent panics (`ValueError`; Python's modular-inverse case for a
    /// negative exponent is not yet supported). Lean refuses.
    PowMod {
        base: Box<Expr>,
        exp: Box<Expr>,
        modulus: Box<Expr>,
    },
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
    /// `set(xs)` — materialise a list into a `HashSet` (de-duplicating).
    /// PMAT-502cw (Tranche 2). Rust/Ruchy emit `(<list>).iter().cloned()
    /// .collect::<std::collections::HashSet<_>>()` (element type inferred).
    /// Result types as `set[T]` over the list's element type. Lean refuses.
    SetFromList { list: Box<Expr> },
    /// `list(<set>)` / `sorted(<set>)` — materialise a `HashSet` back into a
    /// `Vec` (the unique elements, arbitrary order). PMAT-520. Rust/Ruchy emit
    /// `(<set>).iter().cloned().collect::<Vec<_>>()`. Result types as `list[T]`
    /// over the set's element type. Lean refuses.
    SetToList { set: Box<Expr> },
    /// `dict(pairs)` — materialise a list of 2-tuples into a `HashMap`.
    /// PMAT-502dk (Tranche 2). Rust/Ruchy emit `(<pairs>).iter().cloned()
    /// .collect::<std::collections::HashMap<_, _>>()`. Result types as
    /// `dict[K, V]` over the pair list's `tuple[K, V]` element. Also covers
    /// `dict(zip(a, b))` / `dict(enumerate(xs))` (those produce 2-tuple
    /// lists). Lean refuses. (A later key collision keeps the last value,
    /// matching Python's dict-from-pairs semantics.)
    DictFromPairs { pairs: Box<Expr> },
    /// `{k: v, **d, …}` — a dict literal containing at least one `**`-splat,
    /// possibly mixed with explicit `k: v` entries. PMAT-502dw / PMAT-502dx
    /// (Tranche 2). Each `entries` element is either an explicit pair
    /// (`(Some(k), v)`) or a splatted dict (`(None, d)`). Rust/Ruchy chain the
    /// fragments left-to-right — `std::iter::once((<k>, <v>))` for a pair,
    /// `(<d>).iter().map(|(__k, __v)| (__k.clone(), __v.clone()))` for a splat
    /// — then `.collect::<HashMap<_,_>>()`, so a later entry wins on a key
    /// collision (matching Python). Result types as the first entry's dict
    /// type. `entries` has ≥1 element. Lean refuses.
    DictMerge { entries: Vec<(Option<Expr>, Expr)> },
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
    /// PMAT-684: `start` offsets the index (`enumerate(xs, start)` /
    /// `enumerate(xs, start=N)`); `start == 0` is the bare form. The map adds the
    /// offset via `checked_add` (honoring C-PY-INT-ARITH), mirroring the for-loop
    /// `PairIterKind::Enumerate { start }`. Lean refuses.
    Enumerate { list: Box<Expr>, start: i64 },
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
    /// PMAT-502dh: an optional `default` (Python `min(xs, default=d)`) makes
    /// the empty case return `d` instead of panicking — the emit swaps
    /// `.unwrap()` for `.unwrap_or(<default>)` (and the float branch switches
    /// from the ±∞ fold to `.reduce(f64::min/max).unwrap_or(<default>)`).
    ListMinMax {
        list: Box<Expr>,
        is_max: bool,
        of_float: bool,
        key: Option<SortKey>,
        default: Option<Box<Expr>>,
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

/// PMAT-502cv: the radix for `hex`/`oct`/`bin` (`Expr::IntRadixStr`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Radix {
    /// `hex(n)` → prefix `0x`, lowercase hex (`{:x}`).
    Hex,
    /// `oct(n)` → prefix `0o`, octal (`{:o}`).
    Oct,
    /// `bin(n)` → prefix `0b`, binary (`{:b}`).
    Bin,
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
    /// PMAT-502en: `math.hypot(x, y)` → `(x).hypot(y)` (both f64). Not infix.
    Hypot,
    /// PMAT-502en: `math.atan2(y, x)` → `(y).atan2(x)` (both f64). Not infix.
    Atan2,
    /// PMAT-502en: `math.log(x, base)` (2-arg log) → `(x).log(base)` (both
    /// f64). Not infix. (1-arg `math.log` is natural log — `NumBuiltinOp::Ln`.)
    Log,
}

/// PMAT-498 (Tranche 2): scalar numeric builtins carried by
/// [`Expr::NumBuiltin`]. `Abs` takes 1 arg; `Min`/`Max` are variadic
/// (`>= 2` args, PMAT-502cz) and chain `.min`/`.max` over the tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumBuiltinOp {
    /// `abs(x)` → `(x).abs()`
    Abs,
    /// `min(a, b)` → `(a).min(b)`
    Min,
    /// `max(a, b)` → `(a).max(b)`
    Max,
    /// PMAT-502ek: `math.sqrt(x)` → `(x).sqrt()` (always `f64`).
    Sqrt,
    /// PMAT-502ek: `math.floor(x)` → `(x).floor() as i64` (Python `math.floor`
    /// returns an `int`).
    Floor,
    /// PMAT-502ek: `math.ceil(x)` → `(x).ceil() as i64` (returns an `int`).
    Ceil,
    /// PMAT-502el: `math.sin(x)` → `(x).sin()` (`f64`).
    Sin,
    /// PMAT-502el: `math.cos(x)` → `(x).cos()` (`f64`).
    Cos,
    /// PMAT-502el: `math.tan(x)` → `(x).tan()` (`f64`).
    Tan,
    /// PMAT-502el: `math.exp(x)` → `(x).exp()` (`f64`).
    Exp,
    /// PMAT-502el: `math.log(x)` (natural log) → `(x).ln()` (`f64`).
    Ln,
    /// PMAT-502el: `math.log10(x)` → `(x).log10()` (`f64`).
    Log10,
    /// PMAT-502el: `math.log2(x)` → `(x).log2()` (`f64`).
    Log2,
    /// PMAT-502em: `math.trunc(x)` → `(x).trunc() as i64` (Python `math.trunc`
    /// truncates toward zero and returns an `int`).
    Trunc,
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
    /// PMAT-564: `len(s)` over a **str** → `.chars().count() as i64` (Int, 0
    /// args). Python `len` counts Unicode code points, not UTF-8 bytes, so a
    /// str `len` must NOT use `Expr::Len` (which emits `.len()` = byte length).
    /// Synthesized by the frontend `len()` handler for a str-typed argument.
    CharCount,
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
    /// `.split(sep, maxsplit)` → `.splitn((maxsplit) as usize + 1, &(sep)[..])
    /// .map(|s| s.to_string()).collect::<Vec<String>>()` (List(Str), 2 args).
    /// Python's `maxsplit` caps the number of *splits*, so the part count is
    /// `maxsplit + 1` (Rust `splitn` takes the part count). PMAT-518.
    SplitN,
    /// `.rsplit(sep, maxsplit)` → `.rsplitn((maxsplit) as usize + 1, &(sep)[..])
    /// .map(|s| s.to_string()).collect::<Vec<String>>()` THEN reversed
    /// (List(Str), 2 args). PMAT-644. Like [`SplitN`] but splits from the RIGHT,
    /// capping at `maxsplit` splits; Rust's `rsplitn` yields parts right-to-left,
    /// so the collected Vec is reversed to restore Python's left-to-right order.
    RSplitN,
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
    /// `.replace(old, new, count)` → `.replacen(&(old)[..], &(new)[..],
    /// (count) as usize)` — replace the first `count` occurrences (Str, 3 args).
    /// PMAT-517.
    ReplaceN,
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
    /// `.rfind(sub)` → byte index of the **last** match, or `-1` (**Int**, 1
    /// arg). PMAT-545. The reverse-search mirror of [`StrMethodOp::Find`]:
    /// `.rfind(&(sub)[..]).map(|__i| __i as i64).unwrap_or(-1)`.
    /// (ASCII subset — byte index = char index.)
    Rfind,
    /// `.rindex(sub)` → byte index of the **last** match, or **panic** on
    /// absence (Python `ValueError`). PMAT-545. The reverse-search mirror of
    /// [`StrMethodOp::StrIndex`]: `.rfind(&(sub)[..]).map(|__i| __i as i64)
    /// .expect(…)`. (ASCII subset — byte index = char index.)
    RIndex,
    /// `.isdigit()` → `(!(s).is_empty() && (s).chars().all(|c| c.is_ascii_digit()))`
    /// (**Bool**, 0 args). PMAT-502ag. Python returns `False` for the empty
    /// string, so the empty guard is required (a vacuous `.all()` is `true`).
    IsDigit,
    /// `.isnumeric()` → `(!(s).is_empty() && (s).chars().all(|c| c.is_numeric()))`
    /// (**Bool**, 0 args). PMAT-643. Broader than `isdigit` — Rust's
    /// `char::is_numeric()` covers the Unicode Number categories (Nd/Nl/No),
    /// matching Python's `str.isnumeric()`. Shares the empty-guard "all chars
    /// match" shape with [`StrMethodOp::IsDigit`].
    IsNumeric,
    /// `.isalpha()` → `(!(s).is_empty() && (s).chars().all(|c| c.is_alphabetic()))`
    /// (**Bool**, 0 args). PMAT-502ag.
    IsAlpha,
    /// `.isspace()` → `(!(s).is_empty() && (s).chars().all(|c| c.is_whitespace()))`
    /// (**Bool**, 0 args). PMAT-502ag.
    IsSpace,
    /// `.isalnum()` → `(!(s).is_empty() && (s).chars().all(|c| c.is_alphanumeric()))`
    /// (**Bool**, 0 args). PMAT-502di. Shares the empty-guard "all chars match"
    /// shape with [`StrMethodOp::IsDigit`].
    IsAlnum,
    /// `.isupper()` → `((s).chars().any(|c| c.is_uppercase()) && !(s).chars()
    /// .any(|c| c.is_lowercase()))` (**Bool**, 0 args). PMAT-502di. Python's
    /// rule: at least one cased char AND no lowercase among the cased chars
    /// (so `"A1".isupper()` is `True`, `"".isupper()` is `False`).
    IsUpper,
    /// `.islower()` → the lowercase mirror of [`StrMethodOp::IsUpper`]
    /// (**Bool**, 0 args). PMAT-502di.
    IsLower,
    /// `.isascii()` → `(s).is_ascii()` (**Bool**, 0 args). PMAT-695. True iff
    /// every char is in the ASCII range (U+0000..=U+007F). The empty string is
    /// `True` in both Python and Rust (`"".is_ascii()` is `true`), so — unlike
    /// the `isdigit`-family predicates — no empty guard is needed.
    IsAscii,
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
    /// `.removeprefix(p)` → the string with a leading `p` removed, else
    /// unchanged (**Str**, 1 arg). PMAT-502cq. Emits a block over Rust's
    /// `str::strip_prefix`: `{ let __s = (recv); match __s.strip_prefix(
    /// &(p)[..]) { Some(__r) => __r.to_string(), None => __s } }`.
    RemovePrefix,
    /// `.removesuffix(p)` → trailing `p` removed, else unchanged (**Str**,
    /// 1 arg). PMAT-502cq. Like [`StrMethodOp::RemovePrefix`] with
    /// `strip_suffix`.
    RemoveSuffix,
    /// `.swapcase()` → upper↔lower each char (**Str**, 0 args). PMAT-502cr.
    /// Emits `(<recv>).chars().map(|__c| if __c.is_uppercase() {
    /// __c.to_lowercase().collect::<String>() } else if __c.is_lowercase() {
    /// __c.to_uppercase().collect::<String>() } else { __c.to_string() })
    /// .collect::<String>()` — non-cased chars are left unchanged, matching
    /// Python.
    SwapCase,
    /// `.zfill(width)` → left-pad with `0` to `width` chars, **sign-aware**
    /// (a leading `-`/`+` stays first, zeros are inserted after it)
    /// (**Str**, 1 int arg). PMAT-502cs. A string already ≥ `width` is
    /// returned unchanged. Block-form codegen (receiver used several times).
    ZFill,
    /// `.center(width)` → centre in a field of `width` chars, space-padded
    /// (**Str**, 1 int arg). PMAT-502cu. Matches CPython's parity-dependent
    /// bias `left = marg/2 + (marg & width & 1)` (so `"ab".center(5)` →
    /// `"  ab "`, not Rust `{:^}`'s right-bias). Already-wide → unchanged.
    /// Block-form codegen.
    Center,
    /// `.partition(sep)` → the 3-tuple `(before, sep, after)` split at the
    /// **first** `sep` (**`tuple[str, str, str]`**, 1 arg). PMAT-502dj. Emits
    /// `match (recv).split_once(&(sep)[..]) { Some((__a, __b)) =>
    /// (__a.to_string(), (sep).to_string(), __b.to_string()), None =>
    /// ((recv).to_string(), String::new(), String::new()) }` — when `sep` is
    /// absent Python returns `(s, "", "")`. Block-form codegen.
    Partition,
    /// `.rpartition(sep)` → the 3-tuple split at the **last** `sep`
    /// (**`tuple[str, str, str]`**, 1 arg). PMAT-502dj. Like
    /// [`StrMethodOp::Partition`] but via `rsplit_once`; the absent case
    /// returns `("", "", s)` (empty parts **first**, unlike `partition`).
    RPartition,
    /// `.splitlines()` → split on line boundaries (**`list[str]`**, 0 args).
    /// PMAT-502dl. Matches Python's full boundary set (LF, CR, CRLF, VT, FF,
    /// FS/GS/RS, NEL, LS, PS) — Rust's `str::lines()` only handles LF/CRLF,
    /// so the codegen emits an explicit char-walk. No trailing empty element
    /// for a trailing break (matching Python; `keepends=True` is deferred).
    /// Block-form codegen.
    SplitLines,
    /// `.chars().rev().collect::<String>()` (**Str**, 0 args). PMAT-530. The
    /// target of the `s[::-1]` reverse-slice idiom over a `str` (the list form
    /// `xs[::-1]` already lowers to [`Expr::Reversed`]). Reverses by Unicode
    /// scalar value (`char`), matching Python's codepoint-wise reversal on the
    /// ASCII subset; not normalization-aware (grapheme clusters are out of
    /// scope at v0.2.0, as elsewhere in the string surface).
    Reverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOp {
    /// Numeric negation: `-x`, I64 → I64. Note `i64::MIN`'s negation
    /// overflows; frontends should warn but emit anyway.
    Neg,
    /// Logical not: `not x`, Bool → Bool.
    Not,
    /// PMAT-502fb: bitwise invert: Python `~x`, I64 → I64. Python's `~x` is the
    /// two's-complement complement `-(x + 1)`, which is exactly Rust's `!x` on a
    /// signed integer (`~5 == -6` in both). Emits `!(<operand>)`.
    BitNot,
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

/// PMAT-502ep (Tranche 2): set predicates carried by [`Expr::SetPred`]. All
/// yield `bool`. Each maps to a `HashSet` query method; the proper variants
/// additionally require the sets to differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetPredOp {
    /// `a <= b` / `a.issubset(b)` → `a.is_subset(&b)`.
    Subset,
    /// `a < b` → `a.is_subset(&b) && a != b`.
    ProperSubset,
    /// `a >= b` / `a.issuperset(b)` → `a.is_superset(&b)`.
    Superset,
    /// `a > b` → `a.is_superset(&b) && a != b`.
    ProperSuperset,
    /// `a.isdisjoint(b)` → `a.is_disjoint(&b)`.
    Disjoint,
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
    /// PMAT-555: `xs.sort(reverse=True)` — in-place **descending** sort.
    /// Rust/Ruchy emit `.sort_by(|a, b| b.cmp(a))` (`Vec<i64>`) or
    /// `.sort_by(|a, b| b.partial_cmp(a).unwrap())` (`Vec<f64>`); the reversed
    /// comparator gives Python's `list.sort(reverse=True)` ordering directly.
    SortDesc,
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
    /// Python `//` — floor division (rounds toward −∞). Rust `/` truncates
    /// toward zero and `div_euclid` keeps a non-negative remainder; neither
    /// matches Python for a **negative divisor** (e.g. `-7 // -2` is 3, not 4).
    /// The Rust/Ruchy backends emit the truncating quotient plus a floor
    /// correction (PMAT-538); the BigInt slow path uses `div_floor`.
    FloorDiv,
    /// Python `%` — the result takes the sign of the **divisor** (not the
    /// dividend as Rust `%`, nor always-non-negative as `rem_euclid`). Emitted
    /// as the truncating remainder plus a floor correction (PMAT-538).
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

// ─────────────────────────────────────────────────────────────────────
// PMAT-573: reserved-identifier escaping for the Rust-family backends.
//
// A Python program may legally name a variable / parameter / function
// after a word that is a *Rust* keyword but NOT a Python keyword —
// `type`, `match`, `loop`, `move`, `ref`, `mut`, `box`, `final`, … (and
// lowercase `true`/`false`, which Python spells `True`/`False`). Emitted
// verbatim those break `rustc` ("expected identifier, found keyword
// `type`"), violating the xpile invariant transpile-success ⟹ valid Rust.
//
// The fix is a single IR pre-pass (run by the Rust and Ruchy backends on
// a cloned module before emission) that rewrites every *identifier-
// position* string to the Rust raw form `r#name`. Doing it on the data —
// once, at every binding AND every reference — guarantees the two never
// drift, which a per-emit-site escape could not. The walker is exhaustive
// (no wildcard arm) so a future `Expr`/`Stmt` variant fails to compile
// here until its identifier positions are classified — the completeness
// is compiler-enforced. Raw identifiers are a Rust-family feature (Rust +
// Ruchy share the keyword set and `r#` syntax); Lean uses a different
// keyword set and does not call this.
// ─────────────────────────────────────────────────────────────────────

/// True for Rust 2021 strict + reserved keywords that ARE escapable as a
/// raw identifier (`r#kw`). Excludes the four keywords that cannot be raw
/// (`crate`/`self`/`Self`/`super`) — leaving those unescaped also keeps the
/// special-cased `self` method receiver intact — and the contextual
/// keywords (`union`/`macro_rules`) that are already valid bare identifiers.
/// Including the Rust keywords that are *also* Python keywords (`if`, `for`,
/// `return`, …) is harmless: those can never reach us as a Python
/// identifier, so the arm is never taken.
fn is_rust_raw_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break"
            | "const"
            | "continue"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "static"
            | "struct"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
    )
}

/// Rewrite a single identifier-position string in place: a Rust keyword
/// becomes its raw form `r#kw`; anything else (the overwhelming common
/// case) is left untouched. Idempotent — `r#type` is not itself a keyword.
fn escape_name(name: &mut String) {
    if is_rust_raw_keyword(name) {
        *name = format!("r#{name}");
    }
}

fn escape_sortkey(k: &mut SortKey) {
    escape_name(&mut k.param);
    escape_expr(&mut k.body);
}

/// Escape every identifier-position string reachable from `e`.
fn escape_expr(e: &mut Expr) {
    match e {
        // The core reference site.
        Expr::Ident(name) => escape_name(name),
        // Direct call-by-name — escape the callee so it matches the
        // (escaped) function definition name.
        Expr::Call { callee, args } => {
            escape_name(callee);
            for a in args {
                escape_expr(a);
            }
        }
        // Leaves: no sub-expression and no identifier field. (Enum/struct/
        // method/field names are type-level and left unescaped — a Python
        // class/field named after a keyword is a separate, rarer case.)
        Expr::Unit
        | Expr::LitInt(_)
        | Expr::LitFloat(_)
        | Expr::LitBool(_)
        | Expr::LitStr(_)
        | Expr::QuotedString { .. }
        | Expr::ShellVar(_)
        | Expr::ShellSpecial(_)
        | Expr::EnumVariant { .. } => {}
        Expr::Block(b) => {
            for s in &mut b.stmts {
                escape_stmt(s);
            }
            escape_expr(&mut b.trailing_return);
        }
        Expr::FloatBinOp { lhs, rhs, .. }
        | Expr::BinOp { lhs, rhs, .. }
        | Expr::Concat { lhs, rhs }
        | Expr::ListConcat { lhs, rhs }
        | Expr::SetOp { lhs, rhs, .. }
        | Expr::SetPred { lhs, rhs, .. } => {
            escape_expr(lhs);
            escape_expr(rhs);
        }
        Expr::UnOp { operand, .. } => escape_expr(operand),
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            escape_expr(cond);
            escape_expr(then_expr);
            escape_expr(else_expr);
        }
        Expr::StrFormat { args, .. } | Expr::NumBuiltin { args, .. } => {
            for a in args {
                escape_expr(a);
            }
        }
        Expr::StrCharAt { string, index } => {
            escape_expr(string);
            escape_expr(index);
        }
        Expr::StrChars { string } => escape_expr(string),
        Expr::Ord { value } | Expr::Chr { value } => escape_expr(value),
        Expr::IntRadixStr { value, .. }
        | Expr::IntFromStrRadix { value, .. }
        | Expr::FormatSpec { value, .. }
        | Expr::NumCast { value, .. }
        | Expr::ToStr { value, .. }
        | Expr::ReprStr { value }
        | Expr::RoundToInt { value } => escape_expr(value),
        Expr::StrMethod { recv, args, .. } => {
            escape_expr(recv);
            for a in args {
                escape_expr(a);
            }
        }
        Expr::TupleLit(elems) | Expr::ListLit(elems) | Expr::SetLit(elems) => {
            for el in elems {
                escape_expr(el);
            }
        }
        Expr::TupleIndex { tuple, .. } => escape_expr(tuple),
        Expr::Slice {
            collection, lo, hi, ..
        } => {
            escape_expr(collection);
            if let Some(l) = lo {
                escape_expr(l);
            }
            if let Some(h) = hi {
                escape_expr(h);
            }
        }
        Expr::Sum { list, start, .. } => {
            escape_expr(list);
            if let Some(s) = start {
                escape_expr(s);
            }
        }
        Expr::BoolReduce { list, .. }
        | Expr::Reversed { list }
        | Expr::SetFromList { list }
        | Expr::Enumerate { list, .. } => escape_expr(list),
        Expr::RoundToDigits { value, ndigits } | Expr::RoundIntToDigits { value, ndigits } => {
            escape_expr(value);
            escape_expr(ndigits);
        }
        Expr::Repeat { seq, n, .. } => {
            escape_expr(seq);
            escape_expr(n);
        }
        Expr::Sorted { list, key, .. } => {
            escape_expr(list);
            if let Some(k) = key {
                escape_sortkey(k);
            }
        }
        Expr::Gcd { a, b } | Expr::Lcm { a, b } => {
            escape_expr(a);
            escape_expr(b);
        }
        Expr::Factorial { n } | Expr::Isqrt { n } => escape_expr(n),
        Expr::Comb { n, k } | Expr::Perm { n, k } => {
            escape_expr(n);
            escape_expr(k);
        }
        Expr::PowMod { base, exp, modulus } => {
            escape_expr(base);
            escape_expr(exp);
            escape_expr(modulus);
        }
        Expr::RangeList { start, stop, .. } => {
            escape_expr(start);
            escape_expr(stop);
        }
        Expr::SetToList { set } => escape_expr(set),
        Expr::DictFromPairs { pairs } => escape_expr(pairs),
        Expr::DictMerge { entries } => {
            for (k, v) in entries {
                if let Some(k) = k {
                    escape_expr(k);
                }
                escape_expr(v);
            }
        }
        Expr::Filter { list, lambda } | Expr::Map { list, lambda } => {
            escape_expr(list);
            escape_sortkey(lambda);
        }
        Expr::Zip { left, right } => {
            escape_expr(left);
            escape_expr(right);
        }
        Expr::ListMinMax {
            list, key, default, ..
        } => {
            escape_expr(list);
            if let Some(k) = key {
                escape_sortkey(k);
            }
            if let Some(d) = default {
                escape_expr(d);
            }
        }
        Expr::ListQuery { list, arg, .. } => {
            escape_expr(list);
            escape_expr(arg);
        }
        Expr::ListPop { list, index } => {
            escape_expr(list);
            if let Some(i) = index {
                escape_expr(i);
            }
        }
        Expr::DictPop { dict, key, default } => {
            escape_expr(dict);
            escape_expr(key);
            if let Some(d) = default {
                escape_expr(d);
            }
        }
        Expr::DictSetDefault { dict, key, default } => {
            escape_expr(dict);
            escape_expr(key);
            escape_expr(default);
        }
        Expr::SetContains { set, elem } => {
            escape_expr(set);
            escape_expr(elem);
        }
        Expr::ListContains { list, elem } => {
            escape_expr(list);
            escape_expr(elem);
        }
        Expr::StrContains { haystack, needle } => {
            escape_expr(haystack);
            escape_expr(needle);
        }
        Expr::Clone(inner) | Expr::OptionUnwrap(inner) => escape_expr(inner),
        Expr::OptionExpr(inner) => {
            if let Some(i) = inner {
                escape_expr(i);
            }
        }
        // PMAT-721: recurse into the tested value (the body is synthetic — only
        // `__v` + literals — but recurse defensively).
        Expr::OptionTruthy { value, body, .. } => {
            escape_expr(value);
            escape_expr(body);
        }
        // PMAT-724: recurse into value + default (body is synthetic).
        Expr::OptionOrDefault {
            value,
            body,
            default,
            ..
        } => {
            escape_expr(value);
            escape_expr(body);
            escape_expr(default);
        }
        Expr::IsNone { value, .. } => escape_expr(value),
        Expr::TryCatch { body, handler } => {
            escape_expr(body);
            escape_expr(handler);
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                escape_expr(v);
            }
        }
        Expr::FieldAccess { obj, .. } => escape_expr(obj),
        Expr::MethodCall { obj, args, .. } => {
            escape_expr(obj);
            for a in args {
                escape_expr(a);
            }
        }
        Expr::DictLit(pairs) => {
            for (k, v) in pairs {
                escape_expr(k);
                escape_expr(v);
            }
        }
        Expr::Index { collection, index } => {
            escape_expr(collection);
            escape_expr(index);
        }
        Expr::DictGet { dict, key }
        | Expr::DictGetOpt { dict, key }
        | Expr::DictContains { dict, key } => {
            escape_expr(dict);
            escape_expr(key);
        }
        Expr::DictGetOr { dict, key, default } => {
            escape_expr(dict);
            escape_expr(key);
            escape_expr(default);
        }
        Expr::DictView { dict, .. } => escape_expr(dict),
        Expr::Len(inner) => escape_expr(inner),
        Expr::CommandSubstitution(inner) => escape_stmt(inner),
    }
}

/// Escape every identifier-position string reachable from `s`.
fn escape_stmt(s: &mut Stmt) {
    match s {
        Stmt::Return(e) => escape_expr(e),
        Stmt::Let { name, value, .. } | Stmt::Assign { name, value } => {
            escape_name(name);
            escape_expr(value);
        }
        Stmt::ClosureLet { name, params, body } => {
            escape_name(name);
            for (p, _) in params {
                escape_name(p);
            }
            escape_expr(body);
        }
        Stmt::LetTuple { names, value, .. } => {
            for n in names {
                escape_name(n);
            }
            escape_expr(value);
        }
        Stmt::While { cond, body } => {
            escape_expr(cond);
            for st in body {
                escape_stmt(st);
            }
        }
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            escape_expr(cond);
            for st in then_body {
                escape_stmt(st);
            }
            for st in else_body {
                escape_stmt(st);
            }
        }
        Stmt::Continue | Stmt::Break => {}
        Stmt::Print { args, .. } => {
            for a in args {
                escape_expr(a);
            }
        }
        Stmt::ForEach {
            var, iter, body, ..
        } => {
            escape_name(var);
            escape_expr(iter);
            for st in body {
                escape_stmt(st);
            }
        }
        Stmt::ForEachPair {
            first,
            second,
            iter,
            kind,
            body,
        } => {
            escape_name(first);
            escape_name(second);
            escape_expr(iter);
            if let PairIterKind::Zip(other) = kind {
                escape_expr(other);
            }
            for st in body {
                escape_stmt(st);
            }
        }
        Stmt::ForEachZip3 {
            first,
            second,
            third,
            iter1,
            iter2,
            iter3,
            body,
        } => {
            escape_name(first);
            escape_name(second);
            escape_name(third);
            escape_expr(iter1);
            escape_expr(iter2);
            escape_expr(iter3);
            for st in body {
                escape_stmt(st);
            }
        }
        Stmt::ListAppend { list_name, elem }
        | Stmt::SetAdd {
            set_name: list_name,
            elem,
        } => {
            escape_name(list_name);
            escape_expr(elem);
        }
        Stmt::SetRemove { set_name, elem, .. } => {
            escape_name(set_name);
            escape_expr(elem);
        }
        Stmt::ListMutate { list_name, .. } => escape_name(list_name),
        Stmt::ListExtend {
            list_name,
            other: value,
        }
        | Stmt::DictUpdate {
            dict_name: list_name,
            other: value,
        }
        | Stmt::ListRemoveValue { list_name, value } => {
            escape_name(list_name);
            escape_expr(value);
        }
        Stmt::ListInsert {
            list_name,
            index,
            elem,
        } => {
            escape_name(list_name);
            escape_expr(index);
            escape_expr(elem);
        }
        Stmt::IndexAssign {
            list_name,
            indices,
            value,
        } => {
            escape_name(list_name);
            for i in indices {
                escape_expr(i);
            }
            escape_expr(value);
        }
        Stmt::DictSet {
            dict_name,
            key,
            value,
        } => {
            escape_name(dict_name);
            escape_expr(key);
            escape_expr(value);
        }
        Stmt::IndexAppend {
            base, index, elem, ..
        } => {
            escape_name(base);
            escape_expr(index);
            escape_expr(elem);
        }
        Stmt::DictSetdefaultAppend {
            dict,
            key,
            default,
            elem,
        } => {
            escape_name(dict);
            escape_expr(key);
            escape_expr(default);
            escape_expr(elem);
        }
        Stmt::NestedSubscriptAssign { base, steps, value } => {
            escape_name(base);
            for (i, _) in steps {
                escape_expr(i);
            }
            escape_expr(value);
        }
        Stmt::FieldAssign { obj, value, .. } => {
            escape_name(obj);
            escape_expr(value);
        }
        Stmt::DelItem { name, key, .. } => {
            escape_name(name);
            escape_expr(key);
        }
        Stmt::Assert { cond, msg } => {
            escape_expr(cond);
            if let Some(m) = msg {
                escape_expr(m);
            }
        }
        Stmt::Raise { message } => escape_expr(message),
        // Shell-domain statements carry POSIX names (`Cmd.program`,
        // `ShellAssign.name`, `LoopKind::For.var`), not Rust identifiers;
        // the Rust/Ruchy backends refuse them outright. Leave the names and
        // recurse into composed sub-statements / exprs for completeness.
        Stmt::Cmd { args, .. } => {
            for a in args {
                escape_expr(a);
            }
        }
        Stmt::Pipeline { stages } => {
            for st in stages {
                escape_stmt(st);
            }
        }
        Stmt::ShellAssign { value, .. } => escape_expr(value),
        Stmt::ShellLoop { kind, body } => {
            match kind {
                LoopKind::For { items, .. } => {
                    for it in items {
                        escape_expr(it);
                    }
                }
                LoopKind::While { cond } | LoopKind::Until { cond } => escape_expr(cond),
            }
            for st in body {
                escape_stmt(st);
            }
        }
    }
}

fn escape_function(f: &mut Function, escape_fn_name: bool) {
    if escape_fn_name {
        escape_name(&mut f.name);
    }
    for p in &mut f.params {
        escape_name(&mut p.name);
    }
    for s in &mut f.body.stmts {
        escape_stmt(s);
    }
    escape_expr(&mut f.body.trailing_return);
}

/// PMAT-573: rewrite every identifier in `module` that collides with a Rust
/// keyword to its raw form `r#kw`, so Python locals/params/functions named
/// `type`/`match`/`loop`/… emit valid Rust (and Ruchy). The Rust-family
/// backends call this on a *cloned* module before emission. Struct/enum
/// type names, struct field names, and method names are left unescaped (a
/// keyword-named class/field/method is a separate, rarer fidelity gap).
pub fn escape_rust_reserved_idents(module: &mut Module) {
    for item in &mut module.items {
        match item {
            Item::Function(f) => escape_function(f, true),
            Item::Const { name, value, .. } => {
                escape_name(name);
                escape_expr(value);
            }
            Item::Struct { methods, .. } => {
                // Escape locals/params inside each method body; the method's
                // own name is left alone to stay consistent with the
                // (also-unescaped) `Expr::MethodCall` callee.
                for m in methods {
                    escape_function(m, false);
                }
            }
            Item::Enum { .. } => {}
        }
    }
}
