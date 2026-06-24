//! Shared Rust emission.
//!
//! Takes meta-HIR as input, emits idiomatic Rust. Language-neutral by
//! design — language-specific quirks (Python's int promotion, C's
//! pointer arithmetic, Ruchy's pipeline operator) are normalized in
//! each frontend before reaching codegen.
//!
//! Exposes both:
//!   * [`emit_module`] — free function, kept stable for callers that
//!     don't want to go through the [`Backend`] trait.
//!   * [`RustBackend`] — a [`Backend`] impl that wraps [`emit_module`]
//!     so Rust dispatches through the same trait as PTX / WGSL / Lean.

use std::fmt::Write;
use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, QuorumStatus, Target};
use xpile_meta_hir::{
    BinOp, Block, DictViewKind, Expr, FloatOp, Function, Item, ListMutateOp, ListQueryOp, Module,
    NumBuiltinOp, Param, Radix, SetOp, SetPredOp, SourceLang, Stmt, StrMethodOp, Type, UnOp,
};

// PMAT-789 (HUNT-V18 EXC-001): the typed-`except` discriminator is now an
// ALLOWLIST (it matches the handler's OWN listed types and re-raises everything
// else), so it no longer needs a roster of "known" builtin exceptions — the
// blocklist `KNOWN_EXC` it consulted (PMAT-731) has been removed. Each `xpile:
// <Type>:` panic prefix is matched directly against the `except` clause's named
// types, so a non-cataloged exception (RuntimeError, a custom error) and an
// untagged panic now correctly propagate past a non-matching `except`.

/// PMAT-502by: escape a string for embedding inside a `format!`/`println!`
/// format-string literal — `{`/`}` are doubled (format escapes), `"`/`\`
/// are backslash-escaped (Rust string-literal escapes), and the common
/// control chars are emitted as `\n`/`\t`/`\r`. Used for `print(sep=…, end=…)`.
fn escape_format_literal(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            // PMAT-748 (HUNT-V14 #3): other C0/DEL control chars → `\u{..}` so a
            // bare CR-class byte in an f-string literal segment can't break the
            // lexer or be normalized away (mirrors `escape_rust_str`).
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}

/// PMAT-477 (R8): the Rust infix symbol for a float arithmetic op.
fn float_op_sym(op: FloatOp) -> &'static str {
    match op {
        FloatOp::Add => "+",
        FloatOp::Sub => "-",
        FloatOp::Mul => "*",
        FloatOp::Div => "/",
        // FloorDiv/Mod/Pow + the method-style math ops are emitted via
        // dedicated formulas, never via this helper — keep the match exhaustive.
        FloatOp::FloorDiv => "//",
        FloatOp::Mod => "%",
        FloatOp::Pow => "**",
        FloatOp::Hypot => "hypot",
        FloatOp::Atan2 => "atan2",
        FloatOp::Log => "log",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("unsupported item: {0}")]
    Unsupported(String),
    #[error("formatting error: {0}")]
    Format(#[from] std::fmt::Error),
}

pub fn emit_module(module: &Module) -> Result<String, CodegenError> {
    // PMAT-573: escape Rust-keyword identifiers (`type`/`match`/`loop`/…) on
    // a cloned IR before emission, so a Python local/param/function named
    // after a Rust keyword produces valid Rust. Rewriting the data once (at
    // every binding AND reference together) keeps the two from drifting.
    let mut module = module.clone();
    xpile_meta_hir::escape_rust_reserved_idents(&mut module);
    let module = &module;
    let mut out = String::new();
    writeln!(
        out,
        "// xpile-generated from {:?} module {}",
        module.source_lang, module.name
    )?;
    writeln!(out)?;
    // PMAT-467 (v0.2.0 Track 2.A): C sources lower with C arithmetic
    // semantics (fixed-width `i32`, wrapping overflow) via an isolated
    // emit path, keeping the Python/Ruchy codegen (i64 + checked /
    // bigint) untouched. Governed by `C-C-INT-ARITH` (substrate queued).
    let is_c = matches!(module.source_lang, SourceLang::C);
    for item in &module.items {
        match item {
            Item::Function(f) => {
                if is_c {
                    emit_c_function(&mut out, f)?;
                } else {
                    emit_function(&mut out, f)?;
                }
            }
            // PMAT-502bj: module-level constant → `const NAME: TY = VALUE;`.
            Item::Const { name, ty, value } => {
                write!(out, "const {name}: ")?;
                emit_type(&mut out, ty)?;
                out.push_str(" = ");
                emit_expr(&mut out, value, /*mode=*/ false)?;
                out.push_str(";\n");
            }
            // PMAT-505a (classes epic, first cut): dataclass → derived struct.
            Item::Struct {
                name,
                fields,
                methods,
                frozen,
                order,
            } => {
                // PMAT-592: a frozen dataclass is hashable in Python, so it may
                // be a dict key / set element — derive `Eq, Hash` (else E0277/
                // E0599). Only when every field type is itself `Eq + Hash`
                // (`i64`/`bool`/`String`); a float field disqualifies it (`f64`
                // is neither `Eq` nor `Hash`).
                // PMAT-592: a frozen dataclass is hashable → derive `Eq, Hash`
                // when every field is itself `Eq + Hash` (`i64`/`bool`/`String`);
                // a float field disqualifies it.
                let all_ord_fields = fields
                    .iter()
                    .all(|(_, ty)| matches!(ty, Type::I64 | Type::Bool | Type::Str));
                let derive_eq_hash = *frozen && all_ord_fields;
                // PMAT-750 (HUNT-V14 #6): `@dataclass(order=True)` over all-Ord-able
                // fields also derives `Ord` (+ `Eq`) so instances can be
                // `.sort()`ed / `sorted()` — `Vec::sort` needs `Ord`, and
                // `PartialOrd` alone is rustc E0277. A float field can't derive
                // `Ord` (`f64: !Ord`), so a float-field `order=True` dataclass keeps
                // `PartialOrd` only (sorting it stays deferred — needs a
                // `sort_by(partial_cmp)` path).
                let derive_ord = *order && all_ord_fields;
                // PMAT-762 (HUNT-V16 DD-01): when the dataclass defines its own
                // `__eq__`, do NOT `#[derive(PartialEq)]` — the structural derive
                // (all fields) silently overrode the user's `==` semantics (e.g.
                // comparing only `self.x`). Emit an `impl PartialEq` that
                // delegates to the user method instead. Also suppress `Eq`/`Hash`:
                // a custom `__eq__` is not guaranteed reflexive/consistent, and a
                // Python class with `__eq__` but no `__hash__` is itself
                // unhashable — so dropping the derives matches Python (and a real
                // use as a dict key then fails loud rather than silently wrong).
                let has_custom_eq = methods.iter().any(|m| m.name == "__eq__");
                // PMAT-777 (HUNT-V17 #3): a custom `__ne__` overrides `!=`. The
                // PMAT-762 `impl PartialEq` only set `fn eq`, so `!=` used the
                // default `!eq()` and the user `__ne__` was dead. `__ne__` is
                // independent of `__eq__` in Python, so it ALSO requires a hand
                // `impl PartialEq` (you can't add `fn ne` to a derive) — suppress
                // the structural derive when either is present.
                let has_custom_ne = methods.iter().any(|m| m.name == "__ne__");
                let custom_eq_impl = has_custom_eq || has_custom_ne;
                // PMAT-769 (HUNT-V16 DD-07): a custom `__lt__` overrides `<` via a
                // generated `impl PartialOrd` (below) — suppress the structural
                // `order=True` PartialOrd/Ord derive (a user `__lt__` + the derived
                // lexicographic order would be two conflicting impls).
                // PMAT-791 (HUNT-V18 #11): a custom `__gt__`/`__ge__`/`__le__`
                // defined WITHOUT `__lt__` ALSO needs a generated `impl PartialOrd`
                // (below) — otherwise `>`/`>=`/`<=` over the struct emit a raw Rust
                // operator on a `PartialEq`-only type (rustc E0369). Pick the
                // highest-priority dunder the class actually defines; any of them
                // suppresses the structural `order=True`/Ord derive (a hand impl +
                // the derived lexicographic order would conflict).
                let order_dunder = ["__lt__", "__gt__", "__ge__", "__le__"]
                    .into_iter()
                    .find(|d| methods.iter().any(|m| m.name == *d));
                let has_order_dunder = order_dunder.is_some();
                // PMAT-808 (HUNT-V22 HASH-01): a class with a custom `__hash__` is
                // used as a set element / dict key / `in` test — those need Rust
                // `Hash` + `Eq`, but the struct derived neither and `__hash__` was
                // dead code (rustc E0277/E0599). Emit an `impl Hash` delegating to
                // the user method (below), and ensure `Eq` (Hash requires it). Only
                // when every field is itself `Eq` (`all_ord_fields` — a float field
                // disqualifies it, like the frozen/derive case). Suppress the
                // structural `Hash` derive so the user method wins.
                let has_custom_hash =
                    methods.iter().any(|m| m.name == "__hash__") && all_ord_fields;
                let mut derives = vec!["Clone", "Debug"];
                if !custom_eq_impl {
                    derives.push("PartialEq");
                    if derive_eq_hash || (derive_ord && !has_order_dunder) || has_custom_hash {
                        derives.push("Eq");
                    }
                    if derive_eq_hash && !has_custom_hash {
                        derives.push("Hash");
                    }
                }
                // PMAT-648: `order=True` → `PartialOrd` (lexicographic by field
                // order = Python's tuple comparison). Sound for any comparable
                // field incl. `f64`. Skipped when a custom order dunder provides its
                // own `impl PartialOrd` (PMAT-769/791).
                if *order && !has_order_dunder {
                    derives.push("PartialOrd");
                }
                if derive_ord && !has_order_dunder {
                    derives.push("Ord");
                }
                writeln!(out, "#[derive({})]", derives.join(", "))?;
                writeln!(out, "pub struct {name} {{")?;
                for (field, ty) in fields {
                    write!(out, "    pub {field}: ")?;
                    emit_type(&mut out, ty)?;
                    out.push_str(",\n");
                }
                out.push_str("}\n");
                // PMAT-760 (HUNT-V15 #6): a dataclass instance in an f-string /
                // `str()` / `print()` emitted `format!("{}", obj)`, but the struct
                // only derives `Debug` → rustc E0277 (no `Display`). Python's
                // dataclass `__repr__` is `ClassName(f1=v1, f2=v2)`. Generate a
                // matching `Display` when every field formats the same in Rust
                // `{}` as Python `repr` — i.e. all fields are `int` (i64 `{}` ==
                // Python int) or `bool` (mapped to `True`/`False`). A field of
                // another type (str needs quotes, float its own repr, nested its
                // own Display) is deferred — such a dataclass stays without
                // `Display` (its f-string use keeps the loud E0277 reject).
                // PMAT-776 (HUNT-V17 #2): when the dataclass defines its own
                // `__str__`, that IS its `str()`/`print()`/f-string rendering —
                // generate a `Display` delegating to it, NOT the PMAT-760
                // field-repr (which silently printed `ClassName(f=v)` and left
                // `__str__` dead code). Takes precedence over the field-repr
                // (and works for ANY field types, since `__str__` owns the
                // formatting). The `__str__` body is emitted in the methods impl.
                let has_str = methods.iter().any(|m| m.name == "__str__");
                if has_str {
                    writeln!(out, "impl std::fmt::Display for {name} {{")?;
                    writeln!(
                        out,
                        "    fn fmt(&self, __f: &mut std::fmt::Formatter) -> std::fmt::Result {{"
                    )?;
                    writeln!(out, "        write!(__f, \"{{}}\", self.__str__())")?;
                    out.push_str("    }\n}\n");
                }
                // PMAT-840 (HUNT-V26 #9): a `float` field also formats the same in
                // the Python dataclass `repr` as `str(float)` does — generate a
                // `Display` for an int/bool/FLOAT dataclass (a `str` field, which
                // needs Python's quoted-and-escaped repr, is still deferred — it
                // keeps the loud E0277). The float field reuses the same
                // CPython-faithful float-repr block as `ToStr{of_float}` (`.0` for
                // whole values, scientific for large/small), via a raw-string
                // template so the embedded Rust needs no escaping.
                // PMAT-841 (HUNT-V26 #9): a `str` field also renders in the
                // dataclass repr (Python quotes it: `P(name='hi')`), via the same
                // `repr(str)` escaping the `ReprStr` path uses.
                let display_eligible = !has_str
                    && fields.iter().all(|(_, ty)| {
                        matches!(ty, Type::I64 | Type::Bool | Type::F64 | Type::Str)
                    });
                if display_eligible {
                    let mut fmt_str = format!("{name}(");
                    let mut args = String::new();
                    for (i, (field, ty)) in fields.iter().enumerate() {
                        if i > 0 {
                            fmt_str.push_str(", ");
                        }
                        // PMAT-810: the repr LABEL shows the Python field name —
                        // strip the `r#` a keyword-named field carries in the IR
                        // (the `self.{field}` ACCESS keeps the raw form).
                        let label = field.strip_prefix("r#").unwrap_or(field);
                        write!(fmt_str, "{label}={{}}")?;
                        match ty {
                            Type::Bool => write!(
                                args,
                                ", if self.{field} {{ \"True\" }} else {{ \"False\" }}"
                            )?,
                            Type::F64 => {
                                let blk = py_float_repr_block(&format!("self.{field}"));
                                write!(args, ", {blk}")?;
                            }
                            Type::Str => {
                                let blk = py_str_repr_block(&format!("self.{field}"));
                                write!(args, ", {blk}")?;
                            }
                            _ => write!(args, ", self.{field}")?,
                        }
                    }
                    fmt_str.push(')');
                    writeln!(out, "impl std::fmt::Display for {name} {{")?;
                    writeln!(
                        out,
                        "    fn fmt(&self, __f: &mut std::fmt::Formatter) -> std::fmt::Result {{"
                    )?;
                    writeln!(out, "        write!(__f, \"{fmt_str}\"{args})")?;
                    out.push_str("    }\n}\n");
                }
                // PMAT-506d: instance methods → an `impl` block.
                if !methods.is_empty() {
                    writeln!(out, "impl {name} {{")?;
                    for m in methods {
                        emit_function(&mut out, m)?;
                    }
                    out.push_str("}\n");
                }
                // PMAT-762 (HUNT-V16 DD-01): a custom `__eq__` becomes the `==`
                // semantics via an `impl PartialEq` delegating to the user method
                // (the structural `#[derive(PartialEq)]` was suppressed above).
                // The generated `__eq__` takes `other` by value, so clone the
                // borrowed RHS. This also makes `x in list` (Vec::contains) use
                // the correct equality (DD-02) with no `in`-lowering change.
                if custom_eq_impl {
                    writeln!(out, "impl PartialEq for {name} {{")?;
                    writeln!(out, "    fn eq(&self, __other: &Self) -> bool {{")?;
                    if has_custom_eq {
                        writeln!(out, "        self.__eq__(__other.clone())")?;
                    } else {
                        // PMAT-777: `__ne__` without `__eq__` — Python's `==` is
                        // still the dataclass's structural equality (all fields),
                        // emitted by hand since the derive was suppressed.
                        if fields.is_empty() {
                            out.push_str("        true\n");
                        } else {
                            out.push_str("        ");
                            for (i, (field, _)) in fields.iter().enumerate() {
                                if i > 0 {
                                    out.push_str(" && ");
                                }
                                write!(out, "self.{field} == __other.{field}")?;
                            }
                            out.push('\n');
                        }
                    }
                    out.push_str("    }\n");
                    // PMAT-777: delegate `!=` to a custom `__ne__` when present
                    // (else the default `!eq()` is correct).
                    if has_custom_ne {
                        writeln!(out, "    fn ne(&self, __other: &Self) -> bool {{")?;
                        writeln!(out, "        self.__ne__(__other.clone())")?;
                        out.push_str("    }\n");
                    }
                    out.push_str("}\n");
                }
                // PMAT-769/791 (HUNT-V16 DD-07 / HUNT-V18 #11): a custom order
                // dunder becomes the `<`/`>`/`<=`/`>=` ordering via a generated
                // `impl PartialOrd` (the structural derive was suppressed above).
                // Rust derives all four operators from `partial_cmp`, so one
                // consistent body suffices; build it from the highest-priority
                // dunder the class defines (Python resolves the missing operators
                // by reflection, so a class with all of `__gt__`/`__ge__`/`__le__`
                // is well-ordered). The dunder takes `other` by value → clone.
                if let Some(d) = order_dunder {
                    let body = match d {
                        // a < b ⟺ a.__lt__(b); a > b ⟺ b.__lt__(a); else equal.
                        "__lt__" => "if self.__lt__(__other.clone()) { Some(std::cmp::Ordering::Less) } else if __other.__lt__(self.clone()) { Some(std::cmp::Ordering::Greater) } else { Some(std::cmp::Ordering::Equal) }",
                        // a > b ⟺ a.__gt__(b); a < b ⟺ b.__gt__(a); else equal.
                        "__gt__" => "if self.__gt__(__other.clone()) { Some(std::cmp::Ordering::Greater) } else if __other.__gt__(self.clone()) { Some(std::cmp::Ordering::Less) } else { Some(std::cmp::Ordering::Equal) }",
                        // a >= b both ways ⟺ equal; only a >= b ⟺ greater; else less.
                        "__ge__" => "if self.__ge__(__other.clone()) { if __other.__ge__(self.clone()) { Some(std::cmp::Ordering::Equal) } else { Some(std::cmp::Ordering::Greater) } } else { Some(std::cmp::Ordering::Less) }",
                        // a <= b both ways ⟺ equal; only a <= b ⟺ less; else greater.
                        _ => "if self.__le__(__other.clone()) { if __other.__le__(self.clone()) { Some(std::cmp::Ordering::Equal) } else { Some(std::cmp::Ordering::Less) } } else { Some(std::cmp::Ordering::Greater) }",
                    };
                    writeln!(out, "impl PartialOrd for {name} {{")?;
                    writeln!(
                        out,
                        "    fn partial_cmp(&self, __other: &Self) -> Option<std::cmp::Ordering> {{"
                    )?;
                    writeln!(out, "        {body}")?;
                    out.push_str("    }\n}\n");
                }
                // PMAT-808 (HUNT-V22 HASH-01): a custom `__hash__` becomes the
                // struct's `Hash` via a generated impl delegating to it, so the
                // type is usable as a `HashSet` element / `HashMap` key (Eq was
                // arranged above). When `==` is also hand-impl'd (custom
                // `__eq__`/`__ne__`), `Eq` couldn't be derived (no derived
                // `PartialEq`) — emit the marker `impl Eq` by hand.
                if has_custom_hash {
                    if custom_eq_impl {
                        writeln!(out, "impl Eq for {name} {{}}")?;
                    }
                    writeln!(out, "impl std::hash::Hash for {name} {{")?;
                    writeln!(
                        out,
                        "    fn hash<__H: std::hash::Hasher>(&self, __state: &mut __H) {{"
                    )?;
                    writeln!(out, "        self.__hash__().hash(__state);")?;
                    out.push_str("    }\n}\n");
                }
            }
            // PMAT-513: a Python `Enum` class → a Rust enum. The discriminants
            // are tracked in the IR but `C.NAME.value` lowers to its literal at
            // the frontend, so the emitted enum needs no explicit `= disc`.
            Item::Enum { name, variants } => {
                out.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n");
                writeln!(out, "pub enum {name} {{")?;
                for (variant, _disc) in variants {
                    writeln!(out, "    {variant},")?;
                }
                out.push_str("}\n");
            }
        }
    }
    Ok(out)
}

fn emit_function(out: &mut String, f: &Function) -> Result<(), CodegenError> {
    emit_contract_citations(out, f)?;
    write!(out, "pub fn {}(", f.name)?;
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            write!(out, ", ")?;
        }
        emit_param(out, p)?;
    }
    write!(out, ") -> ")?;
    emit_type(out, &f.return_type)?;
    writeln!(out, " {{")?;
    let mode = function_bigint_mode(f);
    emit_block(out, &f.body, mode)?;
    writeln!(out, "}}")?;
    Ok(())
}

/// PMAT-012: a function is in BigInt mode if any param is BigInt OR
/// any pre-bound Let has type BigInt OR the return type is BigInt. In
/// BigInt mode, the Rust backend emits `xpile_bigint::BigInt::from(...)`
/// for integer literals and plain infix `+ - * <= ...` for arithmetic
/// (BigInt never overflows, so no `.checked_*().expect(...)`).
fn function_bigint_mode(f: &Function) -> bool {
    if matches!(f.return_type, Type::BigInt) {
        return true;
    }
    if f.params.iter().any(|p| matches!(p.ty, Type::BigInt)) {
        return true;
    }
    fn stmt_has_bigint(s: &Stmt) -> bool {
        match s {
            Stmt::Let { ty, .. } => matches!(ty, Type::BigInt),
            // PMAT-494b: tuple unpacking introduces no BigInt binding
            // (tuples aren't BigInt-typed at first cut).
            Stmt::LetTuple { .. } => false,
            // PMAT-504: a closure binding is never BigInt-typed at v0.2.0.
            Stmt::ClosureLet { .. } => false,
            // PMAT-736: a named inner fn is never BigInt-typed at v0.2.0 (its
            // own i64 params/return drive its arithmetic, independent of the
            // enclosing fn's bigint mode).
            Stmt::NestedFn { .. } => false,
            // PMAT-479 (R10): an early return introduces no BigInt
            // binding (bigint mode is set by params/lets/return type).
            // PMAT-503a: a raise introduces no BigInt binding.
            Stmt::Assign { .. } | Stmt::Assert { .. } | Stmt::Return(_) | Stmt::Raise { .. } => {
                false
            }
            // PMAT-502bk: loop-control statements carry no binding.
            Stmt::Continue | Stmt::Break => false,
            // PMAT-502bw: print() introduces no binding.
            Stmt::Print { .. } => false,
            Stmt::While { body, .. }
            | Stmt::ForEach { body, .. }
            | Stmt::ForEachPair { body, .. }
            | Stmt::ForEachZip3 { body, .. } => body.iter().any(stmt_has_bigint),
            // PMAT-478 (R9): recurse both branches of an if/else.
            Stmt::If {
                then_body,
                else_body,
                ..
            } => then_body.iter().any(stmt_has_bigint) || else_body.iter().any(stmt_has_bigint),
            // PMAT-460: list.append() carries no Type::Let, so no
            // BigInt-mode trigger of its own. PMAT-502ap/aq/ar: in-place
            // list mutators / extend / insert likewise carry no binding.
            Stmt::ListAppend { .. }
            | Stmt::SetAdd { .. }
            | Stmt::SetRemove { .. }
            | Stmt::ListMutate { .. }
            | Stmt::ListExtend { .. }
            | Stmt::DictUpdate { .. }
            | Stmt::ListInsert { .. }
            | Stmt::ListRemoveValue { .. } => false,
            // PMAT-461: indexed assignment same disposition.
            Stmt::IndexAssign { .. } => false,
            // PMAT-730: nested subscript assign carries no Type::Let.
            Stmt::NestedSubscriptAssign { .. } => false,
            // PMAT-533: subscript-receiver append carries no Type::Let.
            Stmt::IndexAppend { .. } => false,
            // PMAT-727: setdefault-append carries no Type::Let.
            Stmt::DictSetdefaultAppend { .. } => false,
            // PMAT-466: dict keyed assignment carries no Type::Let;
            // dict values are int/bool/str at v0.2.0, never BigInt.
            Stmt::DictSet { .. } => false,
            // PMAT-506c: field assignment introduces no binding (no Type::Let).
            Stmt::FieldAssign { .. } => false,
            // PMAT-502at: del coll[key] introduces no binding.
            Stmt::DelItem { .. } => false,
            // PMAT-039: shell commands carry no BigInt operands. They
            // also never reach this Rust-codegen scan in practice
            // (bashrs-frontend produces Shell modules that the Rust
            // backend declines at emit_stmt), but exhaustive match
            // keeps the dispatch boundary explicit.
            Stmt::Cmd { .. } => false,
            // PMAT-041: same disposition as Cmd — Pipeline composes
            // Cmd stages; no BigInt operand reachable.
            Stmt::Pipeline { .. } => false,
            // PMAT-048: ShellLoop is bashrs-domain — no BigInt
            // operand reachable through it.
            Stmt::ShellLoop { .. } => false,
            // PMAT-051: ShellAssign same disposition.
            Stmt::ShellAssign { .. } => false,
        }
    }
    f.body.stmts.iter().any(stmt_has_bigint)
}

/// PMAT-840 (HUNT-V26 #9): the CPython-faithful `repr`/`str` of an f64 as a Rust
/// expression block over `accessor` (the field/value access). Mirrors the
/// `Expr::ToStr { of_float: true }` emission — `.0` for whole values, `e±NN`
/// scientific for exponent `< -4` or `>= 16`, `nan`/`inf`/`-inf` for non-finite.
/// A raw-string template (no brace/quote escaping) with `__ACC__` substituted, so
/// it round-trips the same Rust the str(float) path emits. Used by the dataclass
/// `Display` generator for a float field.
fn py_float_repr_block(accessor: &str) -> String {
    let tmpl = r##"{ let __sf = __ACC__; if __sf.is_nan() { String::from("nan") } else if __sf.is_infinite() { String::from(if __sf < 0.0 { "-inf" } else { "inf" }) } else { let __se = format!("{:e}", __sf); let __ep = __se.find('e').unwrap(); let __ex: i32 = __se[__ep + 1..].parse().unwrap(); if __ex < -4 || __ex >= 16 { format!("{}e{}{:02}", &__se[..__ep], if __ex < 0 { "-" } else { "+" }, __ex.abs()) } else if __sf.fract() == 0.0 { format!("{}.0", __sf) } else { format!("{}", __sf) } } }"##;
    tmpl.replace("__ACC__", accessor)
}

/// PMAT-841 (HUNT-V26 #9): the CPython-faithful `repr` of a `str` as a Rust block
/// over `accessor` — mirrors `Expr::ReprStr`: single-quoted (double if the string
/// has a `'` but no `"`), with `\\`/quote/`\n`/`\r`/`\t` and control-char `\xNN`
/// escaping. Raw-string template with `__ACC__` substituted. Used by the dataclass
/// `Display` generator for a str field — Python's dataclass `repr` quotes them
/// (`P(name='hi')`).
/// TODO(DRY): `emit_expr`'s `ReprStr` / `ToStr{of_float}` inline the same logic;
/// consolidate onto these helpers in a follow-up.
fn py_str_repr_block(accessor: &str) -> String {
    let tmpl = r##"{ let __rs = &(__ACC__); let __q = if __rs.contains('\'') && !__rs.contains('"') { '"' } else { '\'' }; let mut __ro = String::new(); __ro.push(__q); for __rc in __rs.chars() { match __rc { '\\' => { __ro.push('\\'); __ro.push('\\'); } '\n' => { __ro.push('\\'); __ro.push('n'); } '\r' => { __ro.push('\\'); __ro.push('r'); } '\t' => { __ro.push('\\'); __ro.push('t'); } __ec if __ec == __q => { __ro.push('\\'); __ro.push(__ec); } __ec if (__ec as u32) < 0x20 || (__ec as u32) == 0x7f || ((__ec as u32) >= 0x80 && (__ec as u32) <= 0x9f) => { __ro.push('\\'); __ro.push('x'); __ro.push_str(&format!("{:02x}", __ec as u32)); } __ec => __ro.push(__ec) } } __ro.push(__q); __ro }"##;
    tmpl.replace("__ACC__", accessor)
}

/// PMAT-011: emit one `// xpile-contract: <ID>` comment line per
/// contract that governs this function. Matches the mdBook convention
/// from `sub/contract-frontend-trait.md`'s citation grid — same prefix
/// across all text-comment hosts, so a single regex finds them all.
/// Lean uses `@[xpile_contract "<ID>"]` (proper structured attribute);
/// LaTeX uses `\xpileContract{<ID>}{...}`; mdBook + Rust + Ruchy share
/// the comment form.
fn emit_contract_citations(out: &mut String, f: &Function) -> Result<(), CodegenError> {
    for id in f.applicable_contracts() {
        writeln!(out, "// xpile-contract: {id}")?;
    }
    Ok(())
}

fn emit_block(out: &mut String, block: &Block, mode: bool) -> Result<(), CodegenError> {
    for stmt in &block.stmts {
        emit_stmt(out, stmt, mode)?;
    }
    write!(out, "    ")?;
    emit_expr(out, &block.trailing_return, mode)?;
    writeln!(out)?;
    Ok(())
}

fn emit_stmt(out: &mut String, stmt: &Stmt, mode: bool) -> Result<(), CodegenError> {
    emit_stmt_indented(out, stmt, "    ", mode)
}

fn emit_stmt_indented(
    out: &mut String,
    stmt: &Stmt,
    indent: &str,
    mode: bool,
) -> Result<(), CodegenError> {
    match stmt {
        Stmt::Let {
            name,
            ty,
            value,
            mutable,
        } => {
            // PMAT-598: a mutable empty `set()` binding must NOT pin its element
            // type to the guessed-default `HashSet<i64>` — when the set is later
            // `.add()`ed a non-int element (a struct, str, …) the annotation is
            // a lie (E0308). Suppress the explicit annotation so rustc infers
            // the element type from the subsequent `.insert(...)`. Sound only
            // for an empty `SetLit` (its value is a bare `HashSet::new()`, no
            // turbofish) that is mutable (⟹ a later insert/reassign rustc can
            // infer from) and still typed at the guessed `Set(I64)` default
            // (an explicit `set[str]`/`set[T]` annotation is already correct).
            let infer_set_elem = *mutable
                && matches!(value, Expr::SetLit(elems) if elems.is_empty())
                && matches!(ty, Type::Set(inner) if **inner == Type::I64);
            let kw = if *mutable { "let mut" } else { "let" };
            if infer_set_elem {
                write!(out, "{indent}{kw} {name} = ")?;
            } else {
                write!(out, "{indent}{kw} {name}: ")?;
                emit_type(out, ty)?;
                write!(out, " = ")?;
            }
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        Stmt::Assign { name, value } => {
            write!(out, "{indent}{name} = ")?;
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-504: `let <name> = |<params>| { <body> };` — a first-class
        // closure (0+ params). The return type is left to Rust inference.
        Stmt::ClosureLet { name, params, body } => {
            write!(out, "{indent}let {name} = |")?;
            for (i, (p, ty, is_mut)) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                // PMAT-749: a reassigned closure parameter needs `mut` (E0384).
                if *is_mut {
                    out.push_str("mut ");
                }
                write!(out, "{p}: ")?;
                emit_type(out, ty)?;
            }
            out.push_str("| { ");
            emit_expr(out, body, mode)?;
            writeln!(out, " }};")?;
            Ok(())
        }
        // PMAT-736: a named inner fn item — `fn <name>(<params>) -> R { <body> }`.
        // Emitted as a real Rust `fn` (not a closure) so a self-call recurses by
        // name (closures can't reference their own binding → E0425). A nested fn
        // is i64-mode (mode=false): its own params drive its arithmetic, so it
        // uses checked_* like a top-level fn even inside a bigint parent.
        Stmt::NestedFn {
            name,
            params,
            ret,
            body,
        } => {
            write!(out, "{indent}fn {name}(")?;
            for (i, (p, ty, is_mut)) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                // PMAT-749: a reassigned fn parameter needs `mut` (E0384).
                if *is_mut {
                    out.push_str("mut ");
                }
                write!(out, "{p}: ")?;
                emit_type(out, ty)?;
            }
            write!(out, ") -> ")?;
            emit_type(out, ret)?;
            writeln!(out, " {{")?;
            let inner_indent = format!("{indent}    ");
            for st in &body.stmts {
                emit_stmt_indented(out, st, &inner_indent, false)?;
            }
            out.push_str(&inner_indent);
            emit_expr(out, &body.trailing_return, false)?;
            writeln!(out)?;
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
        // PMAT-494b: tuple unpacking → `let (a, b, ...) = <value>;`.
        Stmt::LetTuple {
            names,
            mutable,
            value,
        } => {
            // PMAT-547: mark each unpacked name `mut` per its `mutable` flag.
            // PMAT-662: never prefix the `_` wildcard with `mut` — `mut _` is
            // invalid Rust ("`mut` must be followed by a named binding"). Repeated
            // discards (`a, _, _ = t`) aggregate the `_` count in the mutability
            // pre-walk, so the 2nd+ `_` was wrongly flagged mutable.
            let pat = names
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    if n != "_" && mutable.get(i).copied().unwrap_or(false) {
                        format!("mut {n}")
                    } else {
                        n.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            // PMAT-711: a single-element tuple-unpack (`x, = t`) needs the trailing
            // comma so `let (x,) = …` is a 1-tuple DESTRUCTURE, not `let (x) = …`
            // (mere grouping, which binds the whole tuple to `x` → E0308).
            let trailing = if names.len() == 1 { "," } else { "" };
            write!(out, "{indent}let ({pat}{trailing}) = ")?;
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-479 (R10): early `return <expr>;` (e.g. a guard clause).
        Stmt::Return(e) => {
            write!(out, "{indent}return ")?;
            emit_expr(out, e, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-502bk: loop-control statements.
        Stmt::Continue => {
            writeln!(out, "{indent}continue;")?;
            Ok(())
        }
        Stmt::Break => {
            writeln!(out, "{indent}break;")?;
            Ok(())
        }
        // PMAT-502bw/by: `print(a, b, …, sep=…, end=…)`. Args are joined by
        // `sep` in the format string; `end == "\n"` (Python default) uses
        // `println!` (which appends the newline), any other `end` uses
        // `print!` with `end` appended literally. Bare `print()` →
        // `println!();` (or `print!("…end…")`).
        Stmt::Print { args, sep, end } => {
            let macro_name = if end == "\n" { "println!" } else { "print!" };
            let sep_esc = escape_format_literal(sep);
            let mut fmt = (0..args.len())
                .map(|_| "{}")
                .collect::<Vec<_>>()
                .join(&sep_esc);
            if end != "\n" {
                fmt.push_str(&escape_format_literal(end));
            }
            if args.is_empty() && fmt.is_empty() {
                writeln!(out, "{indent}{macro_name}();")?;
                return Ok(());
            }
            write!(out, "{indent}{macro_name}(\"{fmt}\"")?;
            for a in args {
                out.push_str(", ");
                emit_expr(out, a, mode)?;
            }
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-478 (R9): if/else statement → Rust `if c { … } else { … }`.
        // The `else` block is omitted when `else_body` is empty.
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            write!(out, "{indent}if ")?;
            emit_expr(out, cond, mode)?;
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in then_body {
                emit_stmt_indented(out, s, &inner, mode)?;
            }
            if else_body.is_empty() {
                writeln!(out, "{indent}}}")?;
            } else {
                writeln!(out, "{indent}}} else {{")?;
                for s in else_body {
                    emit_stmt_indented(out, s, &inner, mode)?;
                }
                writeln!(out, "{indent}}}")?;
            }
            Ok(())
        }
        Stmt::While { cond, body } => {
            write!(out, "{indent}while ")?;
            emit_expr(out, cond, mode)?;
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in body {
                emit_stmt_indented(out, s, &inner, mode)?;
            }
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
        // PMAT-458 (v0.2.0 Track 1.B): for-each over a collection.
        // Emit `for var in iter.iter().cloned() { body }` — the
        // .iter().cloned() produces owned elements matching the
        // v0.2.0 owned-value posture (Index already returns .clone(),
        // so the body sees owned values consistently).
        Stmt::ForEach {
            var,
            iter,
            body,
            over_keys,
            dict_guard,
            elem_ty: _,
            mutate_elems,
        } => {
            // PMAT-816 (HUNT-V21 #3/4/8): when the body mutates each element in
            // place, bind `var` by `&mut` via `iter_mut()` (no `.cloned()`) so
            // the mutation reaches the original collection (which the frontend
            // marked `mut`). Takes precedence over the cloned path; not combined
            // with `over_keys`/`dict_guard` (the frontend only sets it for a
            // plain list iterable).
            if *mutate_elems {
                write!(out, "{indent}for {var} in ")?;
                emit_expr(out, iter, mode)?;
                writeln!(out, ".iter_mut() {{")?;
                let inner = format!("{indent}    ");
                for s in body {
                    emit_stmt_indented(out, s, &inner, mode)?;
                }
                writeln!(out, "{indent}}}")?;
                return Ok(());
            }
            // PMAT-472 (R3): a dict iterates keys (`for k in d:`) via
            // `.keys().cloned()`; a list iterates elements via
            // `.iter().cloned()`. Both yield owned values.
            let method = if *over_keys { "keys" } else { "iter" };
            // PMAT-743 (HUNT-V12 V12-8): a dict whose keys were materialized
            // (PMAT-742) because the body mutates it. Guard against a SIZE change
            // during iteration — capture the dict's length before the loop and,
            // after each body, panic if it changed (matching Python's
            // `RuntimeError: dictionary changed size during iteration`). A
            // size-stable value-update leaves the length unchanged, so the guard
            // is silent there.
            if let Some(g) = dict_guard {
                writeln!(out, "{indent}{{ let __dg_n0 = {g}.len();")?;
                write!(out, "{indent}for {var} in ")?;
                emit_expr(out, iter, mode)?;
                writeln!(out, ".{method}().cloned() {{")?;
                let inner = format!("{indent}    ");
                for s in body {
                    emit_stmt_indented(out, s, &inner, mode)?;
                }
                writeln!(
                    out,
                    "{inner}if {g}.len() != __dg_n0 {{ panic!(\"xpile: RuntimeError: dictionary changed size during iteration\"); }}"
                )?;
                writeln!(out, "{indent}}} }}")?;
                return Ok(());
            }
            write!(out, "{indent}for {var} in ")?;
            emit_expr(out, iter, mode)?;
            writeln!(out, ".{method}().cloned() {{")?;
            let inner = format!("{indent}    ");
            for s in body {
                emit_stmt_indented(out, s, &inner, mode)?;
            }
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
        // PMAT-495: paired for-loop. enumerate → `(i as i64, e)`; zip →
        // both iterators `.iter().cloned()`.
        Stmt::ForEachPair {
            first,
            second,
            iter,
            kind,
            body,
        } => {
            write!(out, "{indent}for ({first}, {second}) in ")?;
            emit_expr(out, iter, mode)?;
            match kind {
                xpile_meta_hir::PairIterKind::Enumerate { start } => {
                    // PMAT-502ca: `enumerate(xs, start)` offsets the index.
                    // PMAT-595: the offset add honors C-PY-INT-ARITH (a bare
                    // `+ start` silently wraps for a start near i64::MAX).
                    if *start == 0 {
                        out.push_str(
                            ".iter().cloned().enumerate().map(|(__i, __e)| (__i as i64, __e))",
                        );
                    } else {
                        write!(
                            out,
                            ".iter().cloned().enumerate().map(|(__i, __e)| ((__i as i64).checked_add({start}i64).expect(\"xpile: i64 addition overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"), __e))"
                        )?;
                    }
                }
                xpile_meta_hir::PairIterKind::Zip(other) => {
                    out.push_str(".iter().cloned().zip(");
                    emit_expr(out, other, mode)?;
                    out.push_str(".iter().cloned())");
                }
                // PMAT-502y: iterate a list of 2-tuples, destructuring each.
                xpile_meta_hir::PairIterKind::Pairs => {
                    out.push_str(".iter().cloned()");
                }
            }
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in body {
                emit_stmt_indented(out, s, &inner, mode)?;
            }
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
        // PMAT-562: three-way `zip` → left-nested `.zip()` chain with a nested
        // `((a, b), c)` destructure. `.iter().cloned()` on each (non-consuming,
        // like the 2-way `Zip`); stops at the shortest, matching Python `zip`.
        Stmt::ForEachZip3 {
            first,
            second,
            third,
            iter1,
            iter2,
            iter3,
            body,
        } => {
            write!(out, "{indent}for (({first}, {second}), {third}) in ")?;
            emit_expr(out, iter1, mode)?;
            out.push_str(".iter().cloned().zip(");
            emit_expr(out, iter2, mode)?;
            out.push_str(".iter().cloned()).zip(");
            emit_expr(out, iter3, mode)?;
            out.push_str(".iter().cloned())");
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in body {
                emit_stmt_indented(out, s, &inner, mode)?;
            }
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
        // PMAT-460 (v0.2.0 Track 1.B): Python `xs.append(v)` → Rust
        // `xs.push(v);`. The frontend has already marked `xs` as
        // mutable so the emission type-checks.
        Stmt::ListAppend { list_name, elem } => {
            write!(out, "{indent}{list_name}.push(")?;
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-500b: Python `s.add(x)` → Rust `s.insert(x);`.
        Stmt::SetAdd { set_name, elem } => {
            write!(out, "{indent}{set_name}.insert(")?;
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-502av: Python `s.remove(x)` panics if absent (KeyError) →
        // `assert!(s.remove(&(x)), "…");`; `s.discard(x)` is a silent no-op
        // → `s.remove(&(x));` (the returned bool is discarded).
        Stmt::SetRemove {
            set_name,
            elem,
            error_if_absent,
        } => {
            if *error_if_absent {
                write!(out, "{indent}assert!({set_name}.remove(&(")?;
                emit_expr(out, elem, mode)?;
                writeln!(
                    out,
                    ")), \"xpile: KeyError: set.remove(x): x not in set\");"
                )?;
            } else {
                write!(out, "{indent}{set_name}.remove(&(")?;
                emit_expr(out, elem, mode)?;
                writeln!(out, "));")?;
            }
            Ok(())
        }
        // PMAT-502ap: in-place list mutators `xs.sort()/.reverse()/.clear()`.
        // `Vec<f64>` has no `Ord`, so a float sort uses `sort_by(partial_cmp)`.
        // PMAT-616: a NaN element makes `partial_cmp` return `None`; Python's
        // `sort` does NOT raise on NaN (it produces an undefined-but-non-crashing
        // order), so fall back to `Equal` instead of `.unwrap()` panicking.
        // Identical to `.unwrap()` for all finite floats.
        Stmt::ListMutate {
            list_name,
            op,
            of_float,
        } => {
            match op {
                ListMutateOp::Sort if *of_float => writeln!(
                    out,
                    "{indent}{list_name}.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));"
                )?,
                ListMutateOp::Sort => writeln!(out, "{indent}{list_name}.sort();")?,
                // PMAT-555: descending in-place sort (`sort(reverse=True)`) — a
                // reversed comparator.
                ListMutateOp::SortDesc if *of_float => writeln!(
                    out,
                    "{indent}{list_name}.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));"
                )?,
                ListMutateOp::SortDesc => {
                    writeln!(out, "{indent}{list_name}.sort_by(|a, b| b.cmp(a));")?
                }
                ListMutateOp::Reverse => writeln!(out, "{indent}{list_name}.reverse();")?,
                ListMutateOp::Clear => writeln!(out, "{indent}{list_name}.clear();")?,
            }
            Ok(())
        }
        // PMAT-502aq: `xs.extend(ys)` → `xs.extend((<ys>).iter().cloned());`.
        Stmt::ListExtend { list_name, other } => {
            write!(out, "{indent}{list_name}.extend((")?;
            emit_expr(out, other, mode)?;
            writeln!(out, ").iter().cloned());")?;
            Ok(())
        }
        // PMAT-502bb: `d.update(other)` → merge entries, cloning each
        // (`other` is not consumed, matching Python).
        Stmt::DictUpdate { dict_name, other } => {
            write!(out, "{indent}{dict_name}.extend((")?;
            emit_expr(out, other, mode)?;
            writeln!(
                out,
                ").iter().map(|(__k, __v)| (__k.clone(), __v.clone())));"
            )?;
            Ok(())
        }
        // PMAT-502ar / PMAT-590: `xs.insert(i, x)` clamps the index to
        // CPython `list.insert` semantics (listobject.c `ins1`) instead of
        // emitting a bare `as usize` cast. Python clamps any `i > len` to
        // `len` (append) and normalizes a negative `i` to `len + i`, clamping
        // to `0` if still negative — whereas Rust's `Vec::insert` panics for
        // `i > len` and a negative `i` casts to a huge `usize` that also
        // panics. The clamp block restores parity.
        Stmt::ListInsert {
            list_name,
            index,
            elem,
        } => {
            write!(
                out,
                "{indent}{{ let __n = {list_name}.len() as i64; let mut __i = ("
            )?;
            emit_expr(out, index, mode)?;
            out.push_str("); if __i < 0 { __i += __n; if __i < 0 { __i = 0; } } if __i > __n { __i = __n; } ");
            write!(out, "{list_name}.insert(__i as usize, ")?;
            emit_expr(out, elem, mode)?;
            writeln!(out, "); }}")?;
            Ok(())
        }
        // PMAT-502eg: `xs.remove(x)` → find the first equal element and
        // remove it, panicking (≈ Python `ValueError`) if it isn't present.
        Stmt::ListRemoveValue { list_name, value } => {
            write!(out, "{indent}{{ let __v = ")?;
            emit_expr(out, value, mode)?;
            write!(
                out,
                "; let __p = {list_name}.iter().position(|__e| *__e == __v)\
                 .expect(\"xpile: ValueError: list.remove(x): x not in list\"); \
                 {list_name}.remove(__p); }}"
            )?;
            out.push('\n');
            Ok(())
        }
        // PMAT-461 (v0.2.0 Track 1.B): Python `xs[i] = v` → Rust
        // `xs[i as usize] = v;`. Same `as usize` coercion as
        // Expr::Index; same param-mut threading as ListAppend.
        Stmt::IndexAssign {
            list_name,
            indices,
            value,
        } => {
            // PMAT-640/641: any runtime index (not a non-negative literal) — at
            // ANY nesting level — wraps like Python (`xs[-1] = v`,
            // `grid[i][-1] = v`), mirroring the `Expr::Index` read path. Each
            // level's index is bound to a temp FIRST, using the
            // progressively-indexed collection's own `len` for the wrap (only
            // evaluated when the index is actually negative). Staging the indices
            // first also ends the collection's immutable borrow before the
            // `index_mut` assign — the E0502 the PMAT-560 self-referential
            // desugar (`xs[len(xs) - k] = v`) hit (so this subsumes the old
            // `needs_temps` path). An all-non-negative-literal path (`xs[0] = v`,
            // `grid[0][1] = v`) keeps the bare form below (no wrap, no churn).
            let any_runtime = indices
                .iter()
                .any(|i| !matches!(i, Expr::LitInt(n) if *n >= 0));
            if any_runtime {
                out.push_str(indent);
                out.push_str("{ ");
                for (n, index) in indices.iter().enumerate() {
                    write!(out, "let __ai{n}: i64 = (")?;
                    emit_expr(out, index, mode)?;
                    write!(
                        out,
                        ") as i64; let __aidx{n} = if __ai{n} < 0 {{ {list_name}"
                    )?;
                    for p in 0..n {
                        write!(out, "[__aidx{p} as usize]")?;
                    }
                    write!(out, ".len() as i64 + __ai{n} }} else {{ __ai{n} }}; ")?;
                    // PMAT-863 (HUNT-V30 #3): bounds-check the WRITE path (the read
                    // path already guards) — an out-of-range subscript-assign
                    // index otherwise silently wrote a wrong slot. Mirrors
                    // Python's IndexError, catchable via `except IndexError`.
                    write!(out, "if __aidx{n} < 0 || __aidx{n} as usize >= {list_name}")?;
                    for p in 0..n {
                        write!(out, "[__aidx{p} as usize]")?;
                    }
                    write!(out, ".len() {{ panic!(\"xpile: IndexError: list assignment index out of range\"); }} ")?;
                }
                write!(out, "{list_name}")?;
                for n in 0..indices.len() {
                    write!(out, "[__aidx{n} as usize]")?;
                }
                out.push_str(" = ");
                emit_expr(out, value, mode)?;
                out.push_str("; }");
                writeln!(out)?;
                return Ok(());
            }
            write!(out, "{indent}{list_name}")?;
            for index in indices {
                out.push('[');
                emit_expr(out, index, mode)?;
                out.push_str(" as usize]");
            }
            out.push_str(" = ");
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-730 (HUNT-V10 V10-7): nested subscript assign with a dict level —
        // `d[a][b] = v`, `dm[k][i] = v`. Navigate intermediate levels with a
        // progressive `&mut` reborrow (`get_mut(&k).unwrap()` for dict / a
        // neg-index-wrapped `&mut t[idx]` for list), then assign at the leaf
        // (`.insert(k, v)` for dict / `t[idx] = v` for list). KeyError-on-absent
        // (dict) and Python negative-index wrap (list) are preserved.
        Stmt::NestedSubscriptAssign { base, steps, value } => {
            let n = steps.len();
            // PMAT-833 (HUNT-V26 #3): evaluate the RHS into a temp BEFORE taking
            // `&mut base`. A read-modify-write on a nested dict cell
            // (`d["a"]["x"] = d["a"]["x"] + 5`) reads `base` immutably in the RHS
            // while the `&mut base` borrow is still live → rustc E0502. Binding
            // `__rhs` first ends the RHS's immutable borrow before the mutable
            // walk begins (the single-level `DictSet` and nested-list paths
            // already sequence the value first; this mirrors them).
            write!(out, "{indent}{{ let __rhs = ")?;
            emit_expr(out, value, mode)?;
            write!(out, "; let __t0 = &mut {base}; ")?;
            for (i, (idx, is_dict)) in steps[..n - 1].iter().enumerate() {
                if *is_dict {
                    write!(out, "let __t{} = __t{i}.get_mut(&(", i + 1)?;
                    emit_expr(out, idx, mode)?;
                    out.push_str(")).unwrap(); ");
                } else {
                    write!(out, "let __li{i} = (")?;
                    emit_expr(out, idx, mode)?;
                    write!(out, ") as i64; let __lx{i} = if __li{i} < 0 {{ __t{i}.len() as i64 + __li{i} }} else {{ __li{i} }}; let __t{} = &mut __t{i}[__lx{i} as usize]; ", i + 1)?;
                }
            }
            let (leaf_idx, leaf_is_dict) = &steps[n - 1];
            if *leaf_is_dict {
                write!(out, "__t{}.insert(", n - 1)?;
                emit_expr(out, leaf_idx, mode)?;
                out.push_str(", __rhs); }");
            } else {
                write!(out, "let __ll = (")?;
                emit_expr(out, leaf_idx, mode)?;
                write!(out, ") as i64; let __lx = if __ll < 0 {{ __t{}.len() as i64 + __ll }} else {{ __ll }}; __t{}[__lx as usize] = __rhs; }}", n - 1, n - 1)?;
            }
            writeln!(out)?;
            Ok(())
        }
        // PMAT-466 (v0.2.0 Track 1.C): Python `d[k] = v` → Rust
        // `{ let __v = v; d.insert(k.clone(), __v); }`. Present-key
        // overwrite / absent-key insert matches Python dict assignment.
        //
        // Two subtleties, both about the move-then-borrow hazard of a
        // non-Copy (`String`) key:
        //   1. The value is bound to a temp BEFORE `.insert`, so the
        //      canonical `d[k] = d.get(k, 0) + 1` idiom (value borrows
        //      the key) doesn't move the key out from under its own
        //      value expression (E0382). Binding the value first also
        //      ends the immutable `.get` borrow before the mutable
        //      `.insert` borrow (NLL).
        //   2. The key is `.clone()`d into `.insert` so the caller's key
        //      binding survives a *later* read of the same key (e.g.
        //      `d[k] = …; return d[k]`). For Copy keys (int/bool) the
        //      clone is a no-op move; `rustc` accepts it (the
        //      `clone_on_copy` lint is clippy-only and xpile does not
        //      clippy emitted output).
        Stmt::DictSet {
            dict_name,
            key,
            value,
        } => {
            write!(out, "{indent}{{ let __xpile_dict_val = ")?;
            emit_expr(out, value, mode)?;
            // PMAT-852 (HUNT-V28 #4): parenthesize the key before `.clone()`. A
            // bare-cast key — `len(w)` → `w.chars().count() as i64` — otherwise
            // emitted `… as i64.clone()`, which rustc parses as `as (i64.clone())`
            // ("cast cannot be followed by a method call"). The parens bind the
            // cast first. (Covers both `d[k] = v` and the dict-comp desugar.)
            write!(out, "; {dict_name}.insert((")?;
            emit_expr(out, key, mode)?;
            writeln!(out, ").clone(), __xpile_dict_val); }}")?;
            Ok(())
        }
        // PMAT-533: append on a subscript receiver. List base indexes a
        // mutable place directly (`base[(i) as usize].push(e)`); dict base
        // reaches the value via `get_mut(&(k)).unwrap()` (panic on absent
        // key = Python KeyError).
        Stmt::IndexAppend {
            base,
            index,
            elem,
            base_is_dict,
        } => {
            if *base_is_dict {
                write!(out, "{indent}{base}.get_mut(&(")?;
                emit_expr(out, index, mode)?;
                out.push_str(")).unwrap().push(");
            } else {
                write!(out, "{indent}{base}[(")?;
                emit_expr(out, index, mode)?;
                out.push_str(") as usize].push(");
            }
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-727 (HUNT-V10 V10-8): `d.setdefault(k, default).append(elem)` →
        // `d.entry(k).or_insert_with(|| default).push(elem);` (creates the entry
        // when absent, unlike IndexAppend's KeyError-panic). `or_insert_with` is
        // lazy; for a `[]` default that is observationally identical to Python's
        // eager default eval.
        Stmt::DictSetdefaultAppend {
            dict,
            key,
            default,
            elem,
        } => {
            write!(out, "{indent}{dict}.entry(")?;
            emit_expr(out, key, mode)?;
            out.push_str(").or_insert_with(|| ");
            emit_expr(out, default, mode)?;
            out.push_str(").push(");
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-506c: struct field assignment `(obj).field = value;`.
        Stmt::FieldAssign { obj, field, value } => {
            write!(out, "{indent}({obj}).{field} = ")?;
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-502at: Python `del coll[key]`. list → `coll.remove((k) as
        // usize);` (shift tail left; panics past end = Python IndexError);
        // dict → `coll.remove(&(k));`.
        // PMAT-709: Python's `del d[k]` raises KeyError on an absent key — the
        // bare `.remove(&k)` discarded the `Option` and silently succeeded
        // (silent-wrong). Assert the removal returned `Some`, mirroring the
        // `Stmt::SetRemove` KeyError assert.
        Stmt::DelItem { name, key, is_dict } => {
            if *is_dict {
                write!(out, "{indent}assert!({name}.shift_remove(&(")?;
                emit_expr(out, key, mode)?;
                writeln!(
                    out,
                    ")).is_some(), \"xpile: KeyError: del d[k]: key not in dict\");"
                )?;
            } else if expr_mentions_ident(key, name) {
                // PMAT-570: `del xs[-k]` → `xs.remove(len(xs) - k)`; the index
                // references `xs`, so bind it before the mutable `remove`. (A
                // literal negative index is frontend-resolved to `len - k`, so it
                // is non-negative here.)
                write!(out, "{indent}{{ let __di = (")?;
                emit_expr(out, key, mode)?;
                writeln!(out, ") as usize; {name}.remove(__di); }}")?;
            } else {
                // PMAT-712: a runtime-negative index must wrap like Python
                // (`i = -1; del xs[i]` removes the last element); the bare
                // `(i) as usize` underflowed a negative to a huge index → panic.
                // Bind + normalize, mirroring the read path (PMAT-639).
                write!(out, "{indent}{{ let __di = (")?;
                emit_expr(out, key, mode)?;
                writeln!(
                    out,
                    ") as i64; let __di = if __di < 0 {{ {name}.len() as i64 + __di }} else {{ __di }}; {name}.remove(__di as usize); }}"
                )?;
            }
            Ok(())
        }
        // PMAT-502ao: `assert cond, msg` → `assert!(cond, "{}", <msg>);`.
        Stmt::Assert { cond, msg } => {
            // PMAT-788 (HUNT-V17 #4): a failed `assert` raises Python
            // `AssertionError`. A bare `assert!(cond, "{}", msg)` panics with an
            // UNTAGGED message, so the typed-`except` discriminator let an
            // unrelated `except ValueError:` SWALLOW it (silent-wrong; Python
            // propagates). Emit a tagged panic so the allowlist discriminator
            // (PMAT-789) lets `except AssertionError` catch it and every other
            // typed `except` re-raise it.
            write!(out, "{indent}if !(")?;
            emit_expr(out, cond, mode)?;
            out.push_str(") { panic!(\"xpile: AssertionError: {}\", ");
            match msg {
                Some(m) => emit_expr(out, m, mode)?,
                None => out.push_str("\"assertion failed\""),
            }
            writeln!(out, "); }}")?;
            Ok(())
        }
        // PMAT-503a: `raise Exc("msg")` → `panic!("{}", <message>);`. The
        // diverging `!` type unifies with any function return, so a `raise`
        // in a guard clause type-checks without a phantom value.
        Stmt::Raise { message } => {
            write!(out, "{indent}panic!(\"{{}}\", ")?;
            emit_expr(out, message, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B: shell-command
        // statements are produced exclusively by bashrs-frontend and
        // consumed exclusively by bashrs-backend. The Rust backend
        // refuses them — there is no meaningful Rust translation of an
        // anonymous shell-line invocation that respects
        // `C-BASHRS-POSIX-IDEMPOTENCE`. (A future cross-domain
        // refinement of `subprocess.run([...])` into a typed
        // `Stmt::Cmd` would still be lowered via Rust's
        // `std::process::Command` API — that's separate machinery, not
        // a generic Cmd-to-Rust translation.)
        Stmt::Cmd { program, args } => Err(CodegenError::Unsupported(format!(
            "Rust backend does not lower Stmt::Cmd (`{program}` with {} arg(s)) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs this construct; \
             use `--target shell` to emit POSIX sh via bashrs-backend",
            args.len()
        ))),
        // PMAT-041: see Cmd arm above. Pipelines have the same
        // cross-domain disposition.
        Stmt::Pipeline { stages } => Err(CodegenError::Unsupported(format!(
            "Rust backend does not lower Stmt::Pipeline ({} stages) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell pipelines; \
             use `--target shell` to emit POSIX sh via bashrs-backend",
            stages.len()
        ))),
        // PMAT-048: same disposition as the rest of the shell domain.
        Stmt::ShellLoop { .. } => Err(CodegenError::Unsupported(
            "Rust backend does not lower Stmt::ShellLoop — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell loops; \
             use `--target shell`"
                .into(),
        )),
        // PMAT-051: same disposition.
        Stmt::ShellAssign { name, .. } => Err(CodegenError::Unsupported(format!(
            "Rust backend does not lower Stmt::ShellAssign (`{name}=…`) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell variable assignment; \
             use `--target shell`"
        ))),
    }
}

fn emit_param(out: &mut String, p: &Param) -> Result<(), CodegenError> {
    // PMAT-506d: a method's `self` receiver emits as `&self` (read-only first
    // cut) — never `self: StructName`.
    if p.name == "self" {
        out.push_str("&self");
        return Ok(());
    }
    // PMAT-460: `mut name: T` for params mutated in-place (currently
    // only via xs.append(v)). Required for Rust to type-check the
    // emitted `name.push(v)`.
    if p.mutable {
        write!(out, "mut ")?;
    }
    write!(out, "{}: ", p.name)?;
    emit_type(out, &p.ty)?;
    Ok(())
}

/// Escape a string for emission inside a Rust `"..."` literal.
/// PMAT-449 (v0.2.0 Track 1.A): minimal escape set for the first
/// `Type::Str` pass — `\` and `"`. Newlines / tabs / unicode escapes
/// land in subsequent sub-tracks alongside f-string lowering.
/// PMAT-747-followup / PMAT-748 (HUNT-V14 #3): escape a Python `str` for
/// emission inside a Rust `"..."` literal. Control bytes MUST be escaped, not
/// passed through raw: a bare CR (`"\r"`) is a hard rustc error ("bare CR not
/// allowed in string"), and a raw CRLF is normalized by Rust's lexer to a lone
/// LF — silently dropping the CR (wrong `len`, wrong bytes for protocol/Windows
/// strings). Emit the named escapes for the common control chars and a
/// `\u{..}` escape for every other C0/DEL control char so the emitted literal
/// is always valid ASCII source carrying the exact code points.
fn escape_rust_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
            }
            other => out.push(other),
        }
    }
    out
}

fn emit_type(out: &mut String, t: &Type) -> Result<(), CodegenError> {
    match t {
        Type::I64 => out.push_str("i64"),
        // PMAT-909: a C `long`/`int64_t` — a distinct 64-bit-ABI width,
        // rendered as Rust `i64` (value-compatible with I64; the C ABI
        // distinction lives in xpile-ffi-manifest's `c_abi_type`).
        Type::CLong => out.push_str("i64"),
        // PMAT-477 (R8): Python `float` → Rust `f64`.
        Type::F64 => out.push_str("f64"),
        Type::Bool => out.push_str("bool"),
        // PMAT-502bl: Python `None` return → Rust unit `()`.
        Type::Unit => out.push_str("()"),
        // PMAT-012: re-exported from `xpile-bigint` (which wraps
        // `num_bigint::BigInt`). Operator overloads (`+`, `-`, `*`,
        // `<=`, …) work without method calls, matching the i64 codegen
        // shape — except no `.checked_*().expect(...)` since BigInt
        // never overflows.
        Type::BigInt => out.push_str("xpile_bigint::BigInt"),
        // PMAT-449: v0.2.0 Track 1.A — Python `str` → Rust owned
        // `String`. First pass is owned-only; `&str` borrowing is the
        // 1.D stretch sub-track per sub/v0.2.0-depyler-merger.md.
        Type::Str => out.push_str("String"),
        // PMAT-455: v0.2.0 Track 1.B — Python `list[T]` → Rust
        // `Vec<T>`. Owned-first; lifetime-borrowing variants come
        // after Track 1.D `&str` work lands.
        Type::List(elem_ty) => {
            out.push_str("Vec<");
            emit_type(out, elem_ty)?;
            out.push('>');
        }
        // PMAT-462: v0.2.0 Track 1.C — Python `dict[K, V]` → Rust
        // `indexmap::IndexMap<K, V>`. Owned-first. The
        // fully-qualified path avoids requiring callers to add a
        // `use` statement.
        Type::Dict(k_ty, v_ty) => {
            out.push_str("indexmap::IndexMap<");
            emit_type(out, k_ty)?;
            out.push_str(", ");
            emit_type(out, v_ty)?;
            out.push('>');
        }
        // PMAT-500: Python `set[T]` → Rust `HashSet<T>`.
        Type::Set(elem_ty) => {
            out.push_str("std::collections::HashSet<");
            emit_type(out, elem_ty)?;
            out.push('>');
        }
        // PMAT-494: Python `tuple[T0, T1, ...]` → Rust `(T0, T1, ...)`.
        Type::Tuple(elems) => {
            out.push('(');
            for (i, t) in elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_type(out, t)?;
            }
            // PMAT-625: a 1-element tuple needs a trailing comma — `(T,)` — else
            // `(T)` is just a parenthesized `T` (not a tuple), so `.0`/indexing
            // fails to compile (E0610).
            if elems.len() == 1 {
                out.push(',');
            }
            out.push(')');
        }
        // PMAT-046: bashrs-domain types. Rust backend refuses — the
        // analogous Rust type for ShellString would be the bashrs
        // runtime's quoting-aware wrapper (not yet shipped); the
        // analogous type for ExitCode is `std::process::ExitStatus`
        // but lowering meta-HIR `Type::ExitCode` to that requires
        // touching the broader `std::process` integration which is
        // XPILE-BASHRS-MERGER-***+. Use `--target shell` instead.
        Type::ShellString | Type::ExitCode => {
            return Err(CodegenError::Unsupported(format!(
                "Rust backend does not lower {t:?} — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs the bashrs type domain; \
                 use `--target shell` for shell-typed signatures"
            )));
        }
        // PMAT-502ew: Python `Optional[T]` → Rust `Option<T>`.
        Type::Optional(inner) => {
            out.push_str("Option<");
            emit_type(out, inner)?;
            out.push('>');
        }
        // PMAT-506b: a struct-typed value emits the bare struct name.
        Type::Struct(name) => out.push_str(name),
    }
    Ok(())
}

/// PMAT-560: does `e` reference the identifier `name`? Used by `IndexAssign` to
/// detect a self-referential index (e.g. the `xs[len(xs) - k]` negative-index
/// desugar), whose immutable borrow of the receiver conflicts with the
/// `index_mut` mutable borrow — such an index is bound to a temp first.
fn expr_mentions_ident(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Ident(n) => n == name,
        Expr::Len(inner) => expr_mentions_ident(inner, name),
        Expr::BinOp { lhs, rhs, .. } | Expr::FloatBinOp { lhs, rhs, .. } => {
            expr_mentions_ident(lhs, name) || expr_mentions_ident(rhs, name)
        }
        Expr::UnOp { operand, .. } => expr_mentions_ident(operand, name),
        Expr::NumCast { value, .. } => expr_mentions_ident(value, name),
        Expr::Index { collection, index } => {
            expr_mentions_ident(collection, name) || expr_mentions_ident(index, name)
        }
        // PMAT-609: recurse into conditional + block forms so a normalized pop
        // index that references the receiver (`{ let __pidx = i; if __pidx < 0 {
        // recv.len() + __pidx } else { __pidx } }`) is detected as
        // self-referential (must be bound before the mutable `remove`).
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_mentions_ident(cond, name)
                || expr_mentions_ident(then_expr, name)
                || expr_mentions_ident(else_expr, name)
        }
        Expr::Block(b) => {
            b.stmts
                .iter()
                .any(|s| matches!(s, Stmt::Let { value, .. } if expr_mentions_ident(value, name)))
                || expr_mentions_ident(&b.trailing_return, name)
        }
        _ => false,
    }
}

fn emit_expr(out: &mut String, e: &Expr, mode: bool) -> Result<(), CodegenError> {
    match e {
        // PMAT-502bl: the unit value (void function trailing return).
        Expr::Unit => out.push_str("()"),
        // PMAT-502dt: a block-expr — `{ <stmts> <trailing> }`.
        Expr::Block(b) => {
            out.push_str("{ ");
            for stmt in &b.stmts {
                emit_stmt(out, stmt, mode)?;
            }
            emit_expr(out, &b.trailing_return, mode)?;
            out.push_str(" }");
        }
        Expr::Ident(name) => {
            // PMAT-013: in BigInt mode, append `.clone()` to every
            // Ident reference. BigInt isn't `Copy`, so an Ident used
            // more than once in a function body (cond + branches,
            // multiplication + recursive call, etc.) would move-on-
            // first-use otherwise. Cloning unconditionally is mechanical
            // and correct; LLVM elides unneeded clones at -O.
            if mode {
                write!(out, "{}.clone()", name)?;
            } else {
                write!(out, "{}", name)?;
            }
        }
        Expr::LitInt(v) => {
            if mode {
                // PMAT-012: literal `n` in a BigInt-mode function is
                // `BigInt::from(<n>i64)`. num-bigint accepts i64 directly.
                write!(out, "xpile_bigint::BigInt::from({}i64)", v)?;
            } else {
                write!(out, "{}i64", v)?;
            }
        }
        // PMAT-477 (R8): float literal → `<v>f64`; float arithmetic →
        // plain infix (IEEE-754 saturates, no checked path).
        Expr::LitFloat(v) => {
            // PMAT-866 (HUNT-V30 #17): a non-finite float literal (`1e400` →
            // inf, also nan) must emit the f64 constant — `{}f64` produced the
            // invalid token `inff64`/`nanf64`. Also unblocks `math.inf`/`nan`.
            if v.is_infinite() {
                out.push_str(if *v < 0.0 {
                    "f64::NEG_INFINITY"
                } else {
                    "f64::INFINITY"
                });
            } else if v.is_nan() {
                out.push_str("f64::NAN");
            } else {
                write!(out, "{v}f64")?;
            }
        }
        Expr::FloatBinOp { op, lhs, rhs } => match op {
            // PMAT-614: Python float floor-division `a // b` is CPython
            // `float_divmod` (Objects/floatobject.c), NOT `(a / b).floor()`.
            // The naive floor over-rounds whenever `a / b` lands just below an
            // integer in float (`1.0 // 0.1` is 9.0 in Python but
            // `(1.0/0.1).floor()` is 10.0), and gives the wrong result for
            // infinite operands (`inf // 2` is `nan`, `-5.0 // inf` is `-1.0`).
            // Replicate CPython exactly: `mod = fmod(a, b)` (Rust `%` IS C
            // `fmod`), `div = (a - mod) / b`, nudge `div` down by 1 when the
            // remainder's sign differs from the divisor's, then `floor(div)`
            // with CPython's `div - floor > 0.5` round-up correction.
            // PMAT-581: guard the zero divisor (Python raises ZeroDivisionError,
            // not `inf`); both operands bound to temps (evaluate-once).
            FloatOp::FloorDiv => {
                out.push_str("{ let __fa: f64 = ");
                emit_expr(out, lhs, mode)?;
                out.push_str("; let __fz: f64 = ");
                emit_expr(out, rhs, mode)?;
                // PMAT-651: when the snapped quotient is zero, CPython's
                // `float_divmod` returns `copysign(0.0, a/b)` — a zero with the
                // sign of the true quotient — so `-0.0 // 1.0` is `-0.0`, not the
                // `+0.0` that `floor(0.0)` yields. Mirror that sign-of-zero branch.
                out.push_str("; if __fz == 0.0 { panic!(\"xpile: ZeroDivisionError: float floor division by zero\"); } let __fm = __fa % __fz; let mut __fd = (__fa - __fm) / __fz; if __fm != 0.0 && ((__fz < 0.0) != (__fm < 0.0)) { __fd -= 1.0; } if __fd != 0.0 { let __ffl = __fd.floor(); if __fd - __ffl > 0.5 { __ffl + 1.0 } else { __ffl } } else { (0.0_f64).copysign(__fa / __fz) } }");
            }
            // PMAT-591: Python float modulo `a % b` is CPython `float_rem`
            // (Objects/floatobject.c): `mod = fmod(a, b)` (Rust's `%` IS C
            // `fmod`), then if `mod != 0` adjust toward the divisor's sign
            // (`mod += b` when their signs differ), else `copysign(0.0, b)`.
            // The earlier floor formula `a - b*(a/b).floor()` (PMAT-502br)
            // introduced an extra rounding step → last-ULP divergence on
            // ~60% of non-power-of-two divisors, and always produced `+0.0`
            // for a zero remainder, losing CPython's divisor-signed zero.
            // PMAT-581: guard the zero divisor (Python raises ZeroDivisionError,
            // not `nan`); bind both operands to temps (evaluate-once).
            FloatOp::Mod => {
                out.push_str("{ let __fz: f64 = ");
                emit_expr(out, rhs, mode)?;
                // PMAT-862 (HUNT-V29 #9): CPython's message is "float modulo by
                // zero" (not the truncated "float modulo").
                out.push_str("; if __fz == 0.0 { panic!(\"xpile: ZeroDivisionError: float modulo by zero\"); } let __fn: f64 = ");
                emit_expr(out, lhs, mode)?;
                out.push_str("; let __r = __fn % __fz; if __r != 0.0 { if (__fz < 0.0) != (__r < 0.0) { __r + __fz } else { __r } } else { 0.0_f64.copysign(__fz) } }");
            }
            // PMAT-734b (HUNT-V11 V11-10): float `b ** e` (`powf`) — CPython raises
            // OverflowError when a FINITE base overflows the float range (e.g.
            // `2.0 ** 2000`), not `inf`; and ZeroDivisionError for `0.0 ** <neg>`
            // (`0.0 ** -1` → inf in Rust). Bind both operands, compute powf, then
            // guard: an infinite result from a finite base is the overflow (or the
            // 0**neg) case. An already-infinite base (`inf ** 2`) keeps `inf`.
            FloatOp::Pow => {
                out.push_str("{ let __pb: f64 = ");
                emit_expr(out, lhs, mode)?;
                out.push_str("; let __pe: f64 = ");
                emit_expr(out, rhs, mode)?;
                out.push_str("; let __pr = __pb.powf(__pe); if __pr.is_infinite() && __pb.is_finite() { if __pb == 0.0 { panic!(\"xpile: ZeroDivisionError: 0.0 cannot be raised to a negative power\"); } panic!(\"xpile: OverflowError: (34, 'Numerical result out of range')\"); } __pr }");
            }
            // PMAT-502bt/em/en: method-style float ops — `(a).<method>(b)`.
            // The 2-arg math functions hypot/atan2/log map 1:1.
            FloatOp::Hypot | FloatOp::Atan2 | FloatOp::Log => {
                let method = match op {
                    FloatOp::Hypot => "hypot",
                    FloatOp::Atan2 => "atan2",
                    FloatOp::Log => "log",
                    _ => unreachable!(),
                };
                out.push('(');
                emit_expr(out, lhs, mode)?;
                write!(out, ").{method}(")?;
                emit_expr(out, rhs, mode)?;
                out.push(')');
            }
            // PMAT-581: float `/` (and int true-division, which lowers to a
            // float Div) raises ZeroDivisionError in Python, not `inf` — guard
            // the divisor.
            FloatOp::Div => {
                // PMAT-862 (HUNT-V29 #9): int/int true division (`1/0`) raises
                // CPython "division by zero"; only a genuinely-float operand gives
                // "float division by zero". Both operands being int→f64 NumCasts
                // means the source `/` was int/int.
                let both_int = matches!(
                    &**lhs,
                    Expr::NumCast {
                        to_float: true,
                        from_float: false,
                        from_str: false,
                        ..
                    }
                ) && matches!(
                    &**rhs,
                    Expr::NumCast {
                        to_float: true,
                        from_float: false,
                        from_str: false,
                        ..
                    }
                );
                let zmsg = if both_int {
                    "division by zero"
                } else {
                    "float division by zero"
                };
                out.push_str("{ let __fz: f64 = ");
                emit_expr(out, rhs, mode)?;
                write!(
                    out,
                    "; if __fz == 0.0 {{ panic!(\"xpile: ZeroDivisionError: {zmsg}\"); }} ("
                )?;
                emit_expr(out, lhs, mode)?;
                out.push_str(") / __fz }");
            }
            FloatOp::Add | FloatOp::Sub | FloatOp::Mul => {
                out.push('(');
                emit_expr(out, lhs, mode)?;
                write!(out, " {} ", float_op_sym(*op))?;
                emit_expr(out, rhs, mode)?;
                out.push(')');
            }
        },
        // PMAT-456 (v0.2.0 Track 1.B): bool literal — Rust's
        // lowercase `true` / `false`.
        Expr::LitBool(b) => write!(out, "{}", b)?,
        Expr::BinOp { op, lhs, rhs } => emit_binop(out, *op, lhs, rhs, mode)?,
        // PMAT-451 (v0.2.0 Track 1.A): str concatenation. Rust's
        // `String + &str` is the idiomatic form but requires the lhs
        // to be owned and rhs to be borrowed — annoying to thread
        // through when both come from the same xpile lowering pipeline.
        // `format!("{}{}", l, r)` works uniformly for any `Display`
        // operands and produces an owned `String`, matching the v0.2.0
        // owned-only ownership posture (see C-XLATE-PY-STR-TO-RUST-STRING
        // `ownership_owned` equation).
        Expr::Concat { lhs, rhs } => {
            out.push_str("format!(\"{}{}\", ");
            emit_expr(out, lhs, mode)?;
            out.push_str(", ");
            emit_expr(out, rhs, mode)?;
            out.push(')');
        }
        // PMAT-502bg: `xs + ys` (lists) → a fresh `Vec` chaining both,
        // consuming neither operand (matching Python).
        Expr::ListConcat { lhs, rhs } => {
            out.push('(');
            emit_expr(out, lhs, mode)?;
            out.push_str(").iter().chain((");
            emit_expr(out, rhs, mode)?;
            out.push_str(").iter()).cloned().collect::<Vec<_>>()");
        }
        // PMAT-502bh: `"<fmt>".format(args…)` → `format!("<fmt>", args…)`.
        // `{fmt:?}` re-escapes the validated format string as a Rust string
        // literal (preserving `{}` placeholders + `{{`/`}}` escapes).
        Expr::StrFormat { fmt, args } => {
            write!(out, "format!({fmt:?}")?;
            for a in args {
                out.push_str(", ");
                emit_expr(out, a, mode)?;
            }
            out.push(')');
        }
        // PMAT-502am: a formatted f-string field → `format!("{:<spec>}", v)`.
        Expr::FormatSpec { value, rust_spec } => {
            // PMAT-659: Rust formats NaN as "NaN", but Python prints "nan". A
            // BARE float-precision spec (`.<digit>`, optionally after a `+`) is
            // float-only (translate_format_spec gates `.Nf` on F64) AND has no
            // width, so the unpadded `"nan"` matches Python. Guard it with
            // `.is_nan()`. The `.`-FILL case (`.<align>`, PMAT-658) is excluded
            // since char-after-`.` is an align char, not a digit. (Width+precision
            // NaN — `8.2` — would need the width applied to "nan"; deferred.) inf
            // already matches ("inf"/"-inf" in both).
            let bare = rust_spec.strip_prefix('+').unwrap_or(rust_spec).as_bytes();
            let is_float_prec =
                bare.first() == Some(&b'.') && bare.get(1).is_some_and(|b| b.is_ascii_digit());
            if is_float_prec {
                out.push_str("{ let __nf = ");
                emit_expr(out, value, mode)?;
                write!(
                    out,
                    "; if __nf.is_nan() {{ String::from(\"nan\") }} else {{ format!(\"{{:{rust_spec}}}\", __nf) }} }}"
                )?;
            } else {
                write!(out, "format!(\"{{:{rust_spec}}}\", ")?;
                emit_expr(out, value, mode)?;
                out.push(')');
            }
        }
        // PMAT-502cd: `s[i]` over a string — materialise the chars and index
        // them (Rust `String` has no positional `[]`). Negative `i` counts
        // from the end. PMAT-801 (HUNT-V19 STR-IDX-OOB): an out-of-range index
        // is Python `IndexError` — bounds-check and panic with the `xpile:
        // IndexError:` tag (a bare `__cs[i]` panics with Rust's untagged "index
        // out of bounds", which the allowlist `except IndexError` can't catch →
        // it wrongly propagated). Mirrors the list-index tagging (PMAT-444/464).
        Expr::StrCharAt { string, index } => {
            out.push_str("{ let __cs: Vec<char> = (");
            emit_expr(out, string, mode)?;
            out.push_str(").chars().collect(); let __i: i64 = (");
            emit_expr(out, index, mode)?;
            out.push_str("); let __idx = if __i < 0 { __cs.len() as i64 + __i } else { __i }; if __idx < 0 || __idx as usize >= __cs.len() { panic!(\"xpile: IndexError: string index out of range\"); } __cs[__idx as usize].to_string() }");
        }
        // PMAT-502cl: string chars as a Vec<String> (for `for c in s`).
        Expr::StrChars { string } => {
            out.push('(');
            emit_expr(out, string, mode)?;
            out.push_str(").chars().map(|__c| __c.to_string()).collect::<Vec<String>>()");
        }
        // PMAT-502cm: ord(c) → code point; chr(n) → 1-char string.
        // PMAT-702: Python's `ord` requires EXACTLY one character — `ord("ab")`
        // raises TypeError (not the first char's code point), `ord("")` likewise.
        // The old `.chars().next().expect(...)` silently returned the FIRST char
        // for a multi-char string. Assert there is no second char. Parenthesized
        // block so it stays a valid expression in any position (`ord(c) + 1`).
        Expr::Ord { value } => {
            // PMAT-725 (HUNT-V10 V10-2): bind the operand in `let __os = &(...)`
            // before `.chars()`. The string-index lowering ends in a `.to_string()`
            // temporary; calling `.chars()` directly on it borrowed a value dropped
            // at the end of the `let __oc = ...` statement (rustc E0716). `&(...)`
            // lifetime-extends an owned temporary to the block AND borrows (does not
            // move) a `String` variable — so `ord(s[0])` and `ord(s)` both compile.
            out.push_str("({ let __os = &(");
            emit_expr(out, value, mode)?;
            out.push_str(
                "); let mut __oc = __os.chars(); let __c0 = __oc.next().expect(\"xpile: ord() expected a character, got an empty string (TypeError)\"); if __oc.next().is_some() { panic!(\"xpile: ord() expected a character (TypeError)\"); } __c0 as i64 })",
            );
        }
        Expr::Chr { value } => {
            out.push_str("char::from_u32((");
            emit_expr(out, value, mode)?;
            out.push_str(") as u32).expect(\"xpile: chr() arg not in range(0x110000) (ValueError)\").to_string()");
        }
        // PMAT-502cv: hex/oct/bin → radix string, sign-first (magnitude via
        // `unsigned_abs` so i64::MIN is safe).
        Expr::IntRadixStr {
            value,
            radix,
            prefixed,
            upper,
            min_width,
        } => {
            out.push_str("{ let __n = (");
            emit_expr(out, value, mode)?;
            out.push_str(
                "); let __m = __n.unsigned_abs(); let __sign = if __n < 0 { \"-\" } else { \"\" }; ",
            );
            // PMAT-502dp: prefix (`0x`/`0o`/`0b`) only when `prefixed`; the
            // hex spec is `{:X}` when `upper`.
            let (prefix, spec) = match radix {
                Radix::Hex if *upper => ("0x", "{:X}"),
                Radix::Hex => ("0x", "{:x}"),
                Radix::Oct => ("0o", "{:o}"),
                Radix::Bin => ("0b", "{:b}"),
            };
            let pfx = if *prefixed { prefix } else { "" };
            if *min_width == 0 {
                write!(out, "format!(\"{{}}{pfx}{spec}\", __sign, __m) }}")?;
            } else {
                // PMAT-773: sign-aware zero-pad — the magnitude (after any prefix)
                // is zero-padded so `len(sign)+len(prefix)+len(digits)` reaches
                // `min_width` (Python counts the sign in the width). Format the
                // digits, then left-pad with '0' to `min_width - sign - prefix`.
                write!(
                    out,
                    "let __body = format!(\"{spec}\", __m); let __pad = ({min_width}usize).saturating_sub(__sign.len() + {pfx_len}); format!(\"{{0}}{pfx}{{1:0>2$}}\", __sign, __body, __pad) }}",
                    pfx_len = pfx.len()
                )?;
            }
        }
        // PMAT-502da: `int(s, base)` → parse via `i64::from_str_radix`
        // (a parse failure / out-of-range digit panics ≈ Python ValueError).
        // PMAT-655: Python `int(s, base)` accepts a base-matching radix PREFIX
        // (`0x`/`0X` for 16, `0o`/`0O` for 8, `0b`/`0B` for 2) and PEP-515
        // underscore digit grouping — Rust's `from_str_radix` accepts neither, so
        // `int("0xff", 16)` / `int("1_000", 16)` panicked. Normalize the string
        // first: trim, peel the optional sign, strip the matching prefix, drop
        // underscores, then parse.
        Expr::IntFromStrRadix { value, radix } => {
            let radix = *radix;
            out.push_str("{ let __ri = &(");
            emit_expr(out, value, mode)?;
            out.push_str("); let __rt = __ri.trim(); let (__rsgn, __rb): (&str, &str) = match __rt.strip_prefix('-') { Some(__r) => (\"-\", __r), None => (\"\", __rt.strip_prefix('+').unwrap_or(__rt)) }; ");
            // PMAT-718 (HUNT-V9 V9-3): validate PEP-515 underscore PLACEMENT before
            // the blanket `replace('_', "")` — Python raises ValueError on a leading,
            // trailing, or doubled underscore (`int("_ff", 16)`, `int("10_", 16)`,
            // `int("1__0", 16)`), whereas the old code silently stripped them and
            // returned a (wrong) value. The check runs on the post-sign, PRE-prefix
            // string so a legal underscore right after the base prefix
            // (`int("0x_ff", 16)` → 255) is preserved (it never starts with `_`).
            out.push_str(&format!(
                "if __rb.starts_with('_') || __rb.ends_with('_') || __rb.contains(\"__\") {{ panic!(\"xpile: ValueError: invalid literal for int() with base {radix}\"); }} "
            ));
            let prefix_strip = match radix {
                16 => "let __rb = __rb.strip_prefix(\"0x\").or_else(|| __rb.strip_prefix(\"0X\")).unwrap_or(__rb); ",
                8 => "let __rb = __rb.strip_prefix(\"0o\").or_else(|| __rb.strip_prefix(\"0O\")).unwrap_or(__rb); ",
                2 => "let __rb = __rb.strip_prefix(\"0b\").or_else(|| __rb.strip_prefix(\"0B\")).unwrap_or(__rb); ",
                _ => "",
            };
            out.push_str(prefix_strip);
            out.push_str("let __rc = format!(\"{}{}\", __rsgn, __rb.replace('_', \"\")); i64::from_str_radix(&__rc, ");
            out.push_str(&format!(
                "{radix}).expect(\"xpile: ValueError: invalid literal for int() with base {radix}\") }}"
            ));
        }
        // PMAT-492/493b: Python string methods. No-arg transforms emit a
        // suffix; the startswith/endswith predicates emit
        // `.starts_with(&(<pat>)[..])` — the `&(..)[..]` reslice yields
        // `&str` uniformly whether the pattern is a `String` or a literal.
        Expr::StrMethod { recv, op, args } => {
            // PMAT-492d: `join` inverts receiver/arg — Python `sep.join(xs)`
            // is Rust `xs.join(sep)` — so emit the list arg as the receiver.
            if matches!(op, StrMethodOp::Join) {
                emit_expr(out, &args[0], mode)?;
                out.push_str(".join(&(");
                emit_expr(out, recv, mode)?;
                out.push_str(")[..])");
            } else if matches!(op, StrMethodOp::IsAscii) {
                // PMAT-695: `.isascii()` → `(s).is_ascii()`. The empty string is
                // `true` in both Python and Rust, so no empty guard (unlike the
                // isdigit-family predicates below).
                out.push('(');
                emit_expr(out, recv, mode)?;
                out.push_str(").is_ascii()");
            } else if matches!(
                op,
                StrMethodOp::IsDigit
                    | StrMethodOp::IsNumeric
                    | StrMethodOp::IsAlpha
                    | StrMethodOp::IsSpace
                    | StrMethodOp::IsAlnum
            ) {
                // PMAT-502ag/502di/643: `.isdigit()`/`.isnumeric()`/`.isalpha()`/
                // `.isspace()`/`.isalnum()` → `(!(s).is_empty() && (s).chars()
                // .all(|__c| __c.<pred>()))`. The empty guard matches Python.
                out.push_str("(!(");
                emit_expr(out, recv, mode)?;
                out.push_str(").is_empty() && (");
                emit_expr(out, recv, mode)?;
                out.push_str(").chars().all(|__c| ");
                out.push_str(match op {
                    StrMethodOp::IsDigit => "__c.is_ascii_digit()",
                    // PMAT-643: Unicode Number categories (Nd/Nl/No), matching
                    // Python `str.isnumeric()` (broader than `isdigit`).
                    StrMethodOp::IsNumeric => "__c.is_numeric()",
                    StrMethodOp::IsAlpha => "__c.is_alphabetic()",
                    StrMethodOp::IsAlnum => "__c.is_alphanumeric()",
                    // PMAT-600: Python `str.isspace()` also treats the C0
                    // information separators FS/GS/RS/US (U+001C..U+001F) as
                    // whitespace, which Rust's `char::is_whitespace()` excludes.
                    _ => "(__c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}'))",
                });
                out.push_str("))");
            } else if matches!(op, StrMethodOp::IsUpper | StrMethodOp::IsLower) {
                // PMAT-502di: `.isupper()` → at least one cased char AND no
                // lowercase among them: `((s).chars().any(|__c|
                // __c.is_uppercase()) && !(s).chars().any(|__c|
                // __c.is_lowercase()))`. `.islower()` is the mirror.
                let (want, forbid) = if matches!(op, StrMethodOp::IsUpper) {
                    ("is_uppercase()", "is_lowercase()")
                } else {
                    ("is_lowercase()", "is_uppercase()")
                };
                out.push_str("((");
                emit_expr(out, recv, mode)?;
                write!(out, ").chars().any(|__c| __c.{want}) && !(")?;
                emit_expr(out, recv, mode)?;
                write!(out, ").chars().any(|__c| __c.{forbid}))")?;
            } else if matches!(op, StrMethodOp::Capitalize) {
                // PMAT-502ah: `.capitalize()` → first char TITLECASED, rest
                // lower (empty → ""), matching Python. PMAT-701: Python uses the
                // titlecase mapping for the lead, not uppercase — `"ß".capitalize()`
                // is "Ss" (titlecase), not "SS" (`to_uppercase`). std has no
                // `char::to_titlecase`, so derive it: keep the first char of the
                // uppercase EXPANSION and lowercase the rest (`ß`→"SS"→"Ss",
                // `ﬂ`→"FL"→"Fl"; a 1-char uppercase is unchanged). The tail uses
                // whole-string `to_lowercase()` (honours the Greek final-sigma rule).
                out.push_str("{ let __cs = &(");
                emit_expr(out, recv, mode)?;
                out.push_str("); let mut __ch = __cs.chars(); match __ch.next() { Some(__f) => { let __ue: String = __f.to_uppercase().collect(); let mut __uec = __ue.chars(); let __lead = match __uec.next() { Some(__h) => __h.to_string() + &__uec.as_str().to_lowercase(), None => String::new() }; __lead + &(__ch.as_str().to_lowercase()) }, None => String::new() } }");
            } else if matches!(op, StrMethodOp::Title) {
                // PMAT-502aj: `.title()` → titlecase the first alpha of each word,
                // lower the rest; any non-alpha is a word boundary (matches
                // Python, incl. `"it's".title()` → `"It'S"`). PMAT-701: the
                // word-start titlecases via the uppercase-expansion (see capitalize)
                // so a titlecase-expanding scalar matches Python (`"ﬂy".title()` →
                // "Fly", not "FLy"). (The per-char tail lowercase still loses the
                // Greek medial-vs-final sigma context — a deferred follow-up.)
                out.push_str("{ let mut __tr = String::new(); let mut __pa = false; for __c in (");
                emit_expr(out, recv, mode)?;
                out.push_str(").chars() { if __c.is_alphabetic() { if __pa { __tr.extend(__c.to_lowercase()); } else { let __ue: String = __c.to_uppercase().collect(); let mut __uec = __ue.chars(); if let Some(__h) = __uec.next() { __tr.push(__h); __tr.push_str(&__uec.as_str().to_lowercase()); } } __pa = true; } else { __tr.push(__c); __pa = false; } } __tr }");
            } else if matches!(op, StrMethodOp::RJust | StrMethodOp::LJust) {
                let is_r = matches!(op, StrMethodOp::RJust);
                if args.len() == 2 {
                    // PMAT-632: `.rjust(w, fill)`/`.ljust(w, fill)` — Rust's
                    // `format!` fill must be a literal, so pad manually by
                    // repeating the fill string to the deficit char count.
                    out.push_str("{ let __s = (");
                    emit_expr(out, recv, mode)?;
                    out.push_str("); let __w = (");
                    emit_expr(out, &args[0], mode)?;
                    // PMAT-666: clamp a negative width to 0 (Python returns the
                    // string unchanged); a bare `as usize` underflowed to a huge
                    // width → capacity-overflow panic.
                    out.push_str(").max(0) as usize; let __n = __s.chars().count(); if __n >= __w { __s } else { let __pad = (");
                    emit_expr(out, &args[1], mode)?;
                    out.push_str(").repeat(__w - __n); ");
                    out.push_str(if is_r {
                        "format!(\"{}{}\", __pad, __s) } }"
                    } else {
                        "format!(\"{}{}\", __s, __pad) } }"
                    });
                } else {
                    // PMAT-502aw: `.rjust(w)`/`.ljust(w)` → `format!("{:>1$}", s, w)`
                    // / `format!("{:<1$}", s, w)`. Rust's format width is a minimum,
                    // so a longer string is returned unchanged (matching Python).
                    out.push_str(if is_r {
                        "format!(\"{:>1$}\", "
                    } else {
                        "format!(\"{:<1$}\", "
                    });
                    emit_expr(out, recv, mode)?;
                    out.push_str(", (");
                    emit_expr(out, &args[0], mode)?;
                    // PMAT-666: clamp a negative width to 0 (see above).
                    out.push_str(").max(0) as usize)");
                }
            } else if matches!(op, StrMethodOp::RemovePrefix | StrMethodOp::RemoveSuffix) {
                // PMAT-502cq: `.removeprefix(p)`/`.removesuffix(p)` →
                // `strip_prefix`/`strip_suffix`, returning the receiver
                // unchanged when the affix is absent (matching Python).
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str(if matches!(op, StrMethodOp::RemovePrefix) {
                    "); match __s.strip_prefix(&("
                } else {
                    "); match __s.strip_suffix(&("
                });
                emit_expr(out, &args[0], mode)?;
                out.push_str(")[..]) { Some(__r) => __r.to_string(), None => __s } }");
            } else if matches!(op, StrMethodOp::ZFill) {
                // PMAT-502cs: `.zfill(w)` → sign-aware zero-pad to `w` chars
                // (a leading -/+ stays first; already-wide strings unchanged).
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let __w = (");
                emit_expr(out, &args[0], mode)?;
                // PMAT-666: clamp a negative width to 0 (Python returns unchanged).
                out.push_str(").max(0) as usize; let __n = __s.chars().count(); if __n >= __w { __s } else { let __pad = \"0\".repeat(__w - __n); if __s.starts_with('-') || __s.starts_with('+') { format!(\"{}{}{}\", &__s[..1], __pad, &__s[1..]) } else { format!(\"{}{}\", __pad, __s) } } }");
            } else if matches!(op, StrMethodOp::Center) {
                // PMAT-502cu: `.center(w)` → space-pad centred, CPython bias
                // `left = marg/2 + (marg & w & 1)` (extra padding parity).
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let __w = (");
                emit_expr(out, &args[0], mode)?;
                // PMAT-666: clamp a negative width to 0 (Python returns unchanged).
                out.push_str(").max(0) as usize; let __n = __s.chars().count(); if __n >= __w { __s } else { let __marg = __w - __n; let __left = __marg / 2 + (__marg & __w & 1); ");
                if args.len() == 2 {
                    // PMAT-632: `.center(w, fill)` — repeat the fill string on
                    // both sides (same CPython left-bias as the space form).
                    out.push_str("let __fc = (");
                    emit_expr(out, &args[1], mode)?;
                    out.push_str("); format!(\"{}{}{}\", __fc.repeat(__left), __s, __fc.repeat(__marg - __left)) } }");
                } else {
                    out.push_str("format!(\"{}{}{}\", \" \".repeat(__left), __s, \" \".repeat(__marg - __left)) } }");
                }
            } else if matches!(op, StrMethodOp::Partition | StrMethodOp::RPartition) {
                // PMAT-502dj: `.partition(sep)` / `.rpartition(sep)` → the
                // 3-tuple `(before, sep, after)` at the first / last `sep`. The
                // absent case differs: partition → `(s, "", "")`, rpartition →
                // `("", "", s)` (matching Python).
                // PMAT-726 (HUNT-V10 V10-1): bind receiver + separator once and
                // guard the empty separator. Python `s.partition("")` raises
                // ValueError('empty separator') at the call (runtime); the old
                // `split_once("")` silently returned `("", "abc")` → wrong 3-tuple
                // with no error (silent-wrong). The runtime guard matches Python's
                // runtime ValueError for both a literal and a dynamic empty sep, and
                // binding `&(...)` avoids re-emitting the separator twice.
                let is_r = matches!(op, StrMethodOp::RPartition);
                out.push_str("{ let __ps = &(");
                emit_expr(out, recv, mode)?;
                out.push_str("); let __psep = &(");
                emit_expr(out, &args[0], mode)?;
                out.push_str("); if __psep.is_empty() { panic!(\"xpile: ValueError: empty separator\"); } match __ps.");
                out.push_str(if is_r { "rsplit_once" } else { "split_once" });
                out.push_str("(__psep.as_str()) { Some((__a, __b)) => (__a.to_string(), __psep.to_string(), __b.to_string()), None => ");
                if is_r {
                    out.push_str("(String::new(), String::new(), __ps.to_string()) } }");
                } else {
                    out.push_str("(__ps.to_string(), String::new(), String::new()) } }");
                }
            } else if matches!(op, StrMethodOp::SplitLines) {
                // PMAT-502dl: `.splitlines()` → split on Python's full line
                // boundary set (Rust's `str::lines()` only covers LF/CRLF), with
                // no trailing empty element for a trailing break. Char-walk.
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let mut __lines: Vec<String> = Vec::new(); let mut __cur = String::new(); let mut __it = __s.chars().peekable(); while let Some(__c) = __it.next() { match __c { '\\r' => { if __it.peek() == Some(&'\\n') { __it.next(); } __lines.push(std::mem::take(&mut __cur)); } '\\n' | '\\u{0b}' | '\\u{0c}' | '\\u{1c}' | '\\u{1d}' | '\\u{1e}' | '\\u{85}' | '\\u{2028}' | '\\u{2029}' => { __lines.push(std::mem::take(&mut __cur)); } _ => __cur.push(__c), } } if !__cur.is_empty() { __lines.push(__cur); } __lines }");
            } else if matches!(
                op,
                StrMethodOp::Find
                    | StrMethodOp::Count
                    | StrMethodOp::StrIndex
                    | StrMethodOp::RIndex
                    | StrMethodOp::Rfind
            ) && args.len() >= 2
            {
                // PMAT-675: `s.find(sub, start[, end])` / `s.count(sub, start[,
                // end])` search within the char-slice `s[start:end]`. `find`
                // returns the CHAR index in the ORIGINAL string (or -1); `count`
                // the number of non-overlapping occurrences. start/end are CHAR
                // indices with Python clamping (negative → +len, then clamp to
                // [0, len]); end defaults to len for the 2-arg form. The slice is
                // a fresh `String` of the selected chars so the byte→char
                // conversion (and the `__st` offset for find) stay correct for
                // non-ASCII.
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let __sub = (");
                emit_expr(out, &args[0], mode)?;
                out.push_str(
                    ").to_string(); let __len = __s.chars().count() as i64; let __st = ((",
                );
                emit_expr(out, &args[1], mode)?;
                out.push_str(") as i64); let __st = (if __st < 0 { __len + __st } else { __st }).clamp(0, __len) as usize; let __en = ");
                if args.len() >= 3 {
                    out.push_str("{ let __e = ((");
                    emit_expr(out, &args[2], mode)?;
                    out.push_str(") as i64); (if __e < 0 { __len + __e } else { __e }).clamp(0, __len) as usize }");
                } else {
                    out.push_str("__len as usize");
                }
                out.push_str("; let __slice: String = __s.chars().skip(__st).take(__en.saturating_sub(__st)).collect(); ");
                // PMAT-854 (HUNT-V28 #11): index/rindex/rfind reuse the same
                // slice+offset; `r*` use `rfind` (rightmost) and `*index` raise
                // ValueError (`.expect`) where `find`/`rfind` return -1.
                if matches!(op, StrMethodOp::Count) {
                    out.push_str("__slice.matches(&__sub[..]).count() as i64 }");
                } else {
                    let finder = if matches!(op, StrMethodOp::Rfind | StrMethodOp::RIndex) {
                        "rfind"
                    } else {
                        "find"
                    };
                    let not_found = if matches!(op, StrMethodOp::StrIndex | StrMethodOp::RIndex) {
                        ".expect(\"xpile: ValueError: substring not found\")"
                    } else {
                        ".unwrap_or(-1)"
                    };
                    write!(out, "__slice.{finder}(&__sub[..]).map(|__b| __st as i64 + __slice[..__b].chars().count() as i64){not_found} }}")?;
                }
            } else if matches!(
                op,
                StrMethodOp::Strip | StrMethodOp::LStrip | StrMethodOp::RStrip
            ) && !args.is_empty()
            {
                // PMAT-691: `s.strip(chars)` / `lstrip` / `rstrip` with a char-SET
                // arg — Python strips any leading/trailing char that is IN `chars`
                // (NOT a substring). Emit `trim_matches` / `trim_start_matches` /
                // `trim_end_matches` with a closure testing membership in the
                // (str) charset (bound once to a temp). The 0-arg whitespace form
                // is handled by the `match op` arms below.
                let trim = match op {
                    StrMethodOp::LStrip => "trim_start_matches",
                    StrMethodOp::RStrip => "trim_end_matches",
                    _ => "trim_matches",
                };
                out.push_str("{ let __cs = (");
                emit_expr(out, &args[0], mode)?;
                out.push_str("); (");
                emit_expr(out, recv, mode)?;
                write!(
                    out,
                    ").{trim}(|__c: char| __cs.contains(__c)).to_string() }}"
                )?;
            } else if matches!(
                op,
                StrMethodOp::Find
                    | StrMethodOp::Rfind
                    | StrMethodOp::StrIndex
                    | StrMethodOp::RIndex
            ) {
                // PMAT-566: `.find/.rfind/.index/.rindex` must return a Python
                // CHARACTER index, not Rust's byte offset. Bind the receiver to a
                // temp (single eval), find the byte offset, then count the chars
                // before it (`__s[..__b].chars().count()`) — `__b` is always a
                // char boundary since it's a match start. `find`/`rfind` →
                // `unwrap_or(-1)`; `index`/`rindex` → `.expect(ValueError)`.
                let finder = if matches!(op, StrMethodOp::Rfind | StrMethodOp::RIndex) {
                    "rfind"
                } else {
                    "find"
                };
                // PMAT-851 (HUNT-V28 #2): bind a CLONE of the receiver — `find`/
                // `rfind`/`index`/`rindex` only read it, but binding `let __s =
                // (recv)` MOVED a non-Copy `String`, so the common `i =
                // s.index(sep); s[i:]` idiom failed rustc E0382 (`s` used after
                // move). The clone keeps the receiver available for later use.
                out.push_str("{ let __s = ((");
                emit_expr(out, recv, mode)?;
                write!(out, ").clone()); __s.{finder}(&(")?;
                emit_expr(out, &args[0], mode)?;
                out.push_str(")[..]).map(|__b| __s[..__b].chars().count() as i64)");
                if matches!(op, StrMethodOp::StrIndex | StrMethodOp::RIndex) {
                    out.push_str(".expect(\"xpile: ValueError: substring not found\") }");
                } else {
                    out.push_str(".unwrap_or(-1) }");
                }
            } else {
                emit_expr(out, recv, mode)?;
                match op {
                    StrMethodOp::Upper => out.push_str(".to_uppercase()"),
                    StrMethodOp::Lower => out.push_str(".to_lowercase()"),
                    // PMAT-600: Python `strip()` removes the C0 separators
                    // U+001C..U+001F too (Rust `trim()` / `char::is_whitespace`
                    // does not) — trim against the Python whitespace predicate.
                    StrMethodOp::Strip => out.push_str(".trim_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
                    // PMAT-564: `len(str)` → Unicode char count (not byte len).
                    StrMethodOp::CharCount => out.push_str(".chars().count() as i64"),
                    // PMAT-530: `s[::-1]` → reverse by Unicode scalar value.
                    StrMethodOp::Reverse => {
                        out.push_str(".chars().rev().collect::<String>()")
                    }
                    // PMAT-502cr: `.swapcase()` → per-char upper↔lower.
                    StrMethodOp::SwapCase => out.push_str(
                        ".chars().map(|__c| if __c.is_uppercase() { __c.to_lowercase().collect::<String>() } else if __c.is_lowercase() { __c.to_uppercase().collect::<String>() } else { __c.to_string() }).collect::<String>()",
                    ),
                    StrMethodOp::StartsWith | StrMethodOp::EndsWith => {
                        out.push_str(if matches!(op, StrMethodOp::StartsWith) {
                            ".starts_with(&("
                        } else {
                            ".ends_with(&("
                        });
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..])");
                    }
                    // PMAT-492c: `.split(sep)` → Vec<String>.
                    StrMethodOp::Split => {
                        out.push_str(".split(&(");
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..]).map(|__c| __c.to_string()).collect::<Vec<String>>()");
                    }
                    // PMAT-518: `.split(sep, maxsplit)` → `.splitn(maxsplit + 1, sep)`.
                    // PMAT-621: a NEGATIVE maxsplit means "no limit" in Python
                    // (split on every occurrence). `(maxsplit as usize) + 1`
                    // WRAPPED for a negative value (`usize::MAX + 1 == 0` →
                    // `splitn(0)` → zero parts); `saturating_add(1)` keeps it at
                    // `usize::MAX` → all parts, matching Python. Positive maxsplit
                    // is unchanged.
                    StrMethodOp::SplitN => {
                        out.push_str(".splitn(((");
                        emit_expr(out, &args[1], mode)?;
                        out.push_str(") as usize).saturating_add(1), &(");
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..]).map(|__c| __c.to_string()).collect::<Vec<String>>()");
                    }
                    // PMAT-644: `.rsplit(sep, maxsplit)` → `.rsplitn(maxsplit + 1,
                    // sep)` (same negative-maxsplit "no limit" via saturating_add)
                    // — but `rsplitn` yields parts right-to-left, so reverse the
                    // collected Vec to restore Python's left-to-right order.
                    StrMethodOp::RSplitN => {
                        out.push_str(".rsplitn(((");
                        emit_expr(out, &args[1], mode)?;
                        out.push_str(") as usize).saturating_add(1), &(");
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..]).map(|__c| __c.to_string()).collect::<Vec<String>>().into_iter().rev().collect::<Vec<String>>()");
                    }
                    // PMAT-502co: no-arg `.split()` → whitespace split.
                    // PMAT-649: Python `str.split()` (no arg) splits on runs of
                    // `str.isspace()` whitespace, which INCLUDES the C0 file/group/
                    // record/unit separators U+001C-1F — but Rust's
                    // `split_whitespace`/`char::is_whitespace` excludes them. Match
                    // PMAT-600's strip/isspace predicate and filter empties to keep
                    // the leading/trailing/consecutive-collapse no-empty semantics.
                    StrMethodOp::SplitWhitespace => {
                        out.push_str(
                            ".split(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).filter(|__c| !__c.is_empty()).map(|__c| __c.to_string()).collect::<Vec<String>>()",
                        );
                    }
                    // PMAT-502b: `.replace(old, new)` → `.replace(&(old)[..], &(new)[..])`.
                    StrMethodOp::Replace => {
                        out.push_str(".replace(&(");
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..], &(");
                        emit_expr(out, &args[1], mode)?;
                        out.push_str(")[..])");
                    }
                    // PMAT-517: `.replace(old, new, count)` → `.replacen(...)`.
                    StrMethodOp::ReplaceN => {
                        out.push_str(".replacen(&(");
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..], &(");
                        emit_expr(out, &args[1], mode)?;
                        out.push_str(")[..], (");
                        emit_expr(out, &args[2], mode)?;
                        out.push_str(") as usize)");
                    }
                    // PMAT-502l: lstrip/rstrip → trim_start/trim_end.
                    // PMAT-600: against the Python whitespace set (incl. the C0
                    // separators U+001C..U+001F).
                    StrMethodOp::LStrip => out.push_str(".trim_start_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
                    StrMethodOp::RStrip => out.push_str(".trim_end_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
                    // PMAT-502l: `.count(sub)` → non-overlapping match count (i64).
                    StrMethodOp::Count => {
                        out.push_str(".matches(&(");
                        emit_expr(out, &args[0], mode)?;
                        out.push_str(")[..]).count() as i64");
                    }
                    // PMAT-566: find/rfind/index/rindex return a CHAR index — block
                    // form handled above (byte offset → chars().count()).
                    StrMethodOp::Find
                    | StrMethodOp::StrIndex
                    | StrMethodOp::Rfind
                    | StrMethodOp::RIndex => {
                        unreachable!("find/rfind/index/rindex handled above")
                    }
                    StrMethodOp::Join => unreachable!("Join handled above"),
                    StrMethodOp::IsDigit
                    | StrMethodOp::IsNumeric
                    | StrMethodOp::IsAlpha
                    | StrMethodOp::IsSpace
                    | StrMethodOp::IsAlnum
                    | StrMethodOp::IsUpper
                    | StrMethodOp::IsLower
                    | StrMethodOp::IsAscii => {
                        unreachable!("classification predicates handled above")
                    }
                    StrMethodOp::Capitalize => unreachable!("capitalize handled above"),
                    StrMethodOp::Title => unreachable!("title handled above"),
                    StrMethodOp::RJust | StrMethodOp::LJust => {
                        unreachable!("rjust/ljust handled above")
                    }
                    StrMethodOp::RemovePrefix | StrMethodOp::RemoveSuffix => {
                        unreachable!("removeprefix/removesuffix handled above")
                    }
                    StrMethodOp::ZFill => unreachable!("zfill handled above"),
                    StrMethodOp::Center => unreachable!("center handled above"),
                    StrMethodOp::Partition | StrMethodOp::RPartition => {
                        unreachable!("partition/rpartition handled above")
                    }
                    StrMethodOp::SplitLines => unreachable!("splitlines handled above"),
                }
            }
        }
        // PMAT-455 (v0.2.0 Track 1.B): Python list literal → Rust
        // `vec![...]` macro. The element types are guaranteed
        // homogeneous by the frontend's lowering check.
        Expr::ListLit(elems) => {
            out.push_str("vec![");
            for (i, e) in elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr(out, e, mode)?;
            }
            out.push(']');
        }
        // PMAT-494: Python tuple literal → Rust `(e0, e1, ...)`.
        Expr::TupleLit(elems) => {
            out.push('(');
            for (i, e) in elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr(out, e, mode)?;
            }
            // PMAT-625: a 1-element tuple literal needs a trailing comma — `(x,)`
            // — else `(x)` is just a parenthesized value, not a tuple.
            if elems.len() == 1 {
                out.push(',');
            }
            out.push(')');
        }
        // PMAT-502q: Python `t[N]` (tuple) → `(<tuple>).N.clone()` — Rust
        // tuple field access, owned-value posture (matches list-index clone).
        Expr::TupleIndex { tuple, index } => {
            out.push('(');
            emit_expr(out, tuple, mode)?;
            write!(out, ").{index}.clone()")?;
        }
        // PMAT-496/539: Python `xs[lo:hi]` slice with full Python bound
        // semantics — a negative bound counts from the end (`+len`), every
        // bound clamps to `[0, len]`, and `lo > hi` yields empty. The naive
        // `(lo) as usize` panicked on a negative bound (wraps to a huge usize)
        // or an out-of-range bound. Emit a block that binds the collection,
        // resolves + clamps each bound, then slices.
        Expr::Slice {
            collection,
            lo,
            hi,
            of_str,
            step,
        } => {
            let resolve = |out: &mut String,
                           bound: &Option<Box<Expr>>,
                           default: &str,
                           mode: bool|
             -> Result<(), CodegenError> {
                match bound {
                    Some(b) => {
                        out.push_str("{ let __b = (");
                        emit_expr(out, b, mode)?;
                        out.push_str(
                            ") as i64; if __b < 0 { (__n + __b).max(0) } else { __b.min(__n) } }",
                        );
                    }
                    None => out.push_str(default),
                }
                Ok(())
            };
            // PMAT-567: a str slice indexes by Unicode CHARACTERS, not bytes —
            // collect to `Vec<char>` so `__n` (char count), the bound clamping,
            // and `__sl[__lo..__hi]` are all char-based (a byte slice gives wrong
            // results AND panics on a char boundary for non-ASCII input). A list
            // slice keeps the by-reference `&Vec` (element-indexed, already right).
            if *of_str {
                out.push_str("{ let __sl: Vec<char> = (");
                emit_expr(out, collection, mode)?;
                out.push_str(").chars().collect(); let __n = __sl.len() as i64; let __lo_i = ");
            } else {
                out.push_str("{ let __sl = &(");
                emit_expr(out, collection, mode)?;
                out.push_str("); let __n = __sl.len() as i64; let __lo_i = ");
            }
            resolve(out, lo, "0", mode)?;
            out.push_str("; let __hi_i = ");
            resolve(out, hi, "__n", mode)?;
            out.push_str("; let __lo = __lo_i as usize; let __hi = __hi_i.max(__lo_i) as usize; ");
            match step {
                // PMAT-548: a negative step `xs[::-k]`/`s[::-k]` reverses then
                // steps (the frontend only sets a negative `step` for the
                // unbounded form, so `__lo..__hi` spans the whole sequence).
                // PMAT-633: for a str, `__sl` is `Vec<char>` — collect the
                // stepped chars back into a String (no `.cloned()` needed; the
                // `&char` iterator collects into String directly).
                Some(s) if *s < 0 => {
                    let k = (-s) as usize;
                    if *of_str {
                        write!(
                            out,
                            "__sl[__lo..__hi].iter().rev().step_by({k}).collect::<String>() }}"
                        )?;
                    } else {
                        write!(
                            out,
                            "__sl[__lo..__hi].iter().rev().step_by({k}).cloned().collect::<Vec<_>>() }}"
                        )?;
                    }
                }
                // PMAT-502bc: a positive step → `.iter().step_by(c)...`;
                // PMAT-633: str collects into String, list into Vec.
                Some(s) => {
                    if *of_str {
                        write!(
                            out,
                            "__sl[__lo..__hi].iter().step_by({s}).collect::<String>() }}"
                        )?;
                    } else {
                        write!(
                            out,
                            "__sl[__lo..__hi].iter().step_by({s}).cloned().collect::<Vec<_>>() }}"
                        )?;
                    }
                }
                None => out.push_str(if *of_str {
                    // PMAT-567: `__sl` is `Vec<char>` for str — collect the slice
                    // back into a String.
                    "__sl[__lo..__hi].iter().collect::<String>() }"
                } else {
                    "__sl[__lo..__hi].to_vec() }"
                }),
            }
        }
        // PMAT-498: scalar numeric builtins → receiver-method form.
        Expr::NumBuiltin { op, args, of_float } => {
            // PMAT-601: float `max`/`min` must follow Python's first-argument-
            // wins semantics (and NaN propagation), NOT `f64::max`/`f64::min`
            // (which treat `+0.0 > -0.0` and silently drop NaN). Emit a left
            // fold: the accumulator starts at args[0]; a later arg replaces it
            // only on a STRICT compare, so a tie (`-0.0`/`0.0`) or a NaN compare
            // (always false) keeps the earlier value, exactly like Python's
            // `result = a; if b > result: result = b`. Integer min/max keep the
            // total-order `.min`/`.max` chain (i64 has no signed-zero/NaN).
            if *of_float && matches!(op, NumBuiltinOp::Min | NumBuiltinOp::Max) {
                let cmp = if matches!(op, NumBuiltinOp::Min) {
                    "<"
                } else {
                    ">"
                };
                out.push_str("{ let mut __m: f64 = ");
                emit_expr(out, &args[0], mode)?;
                out.push(';');
                for arg in &args[1..] {
                    out.push_str(" { let __x: f64 = ");
                    emit_expr(out, arg, mode)?;
                    write!(out, "; if __x {cmp} __m {{ __m = __x; }} }}")?;
                }
                out.push_str(" __m }");
                return Ok(());
            }
            // PMAT-606: `math.floor`/`ceil`/`trunc` return a Python int, so the
            // f64 result is cast to i64. A bare `as i64` SATURATES (since Rust
            // 1.45): `1e30.floor() as i64` → i64::MAX (silent), `inf` → i64::MAX,
            // `nan` → 0 — but Python returns an exact bignum for a huge float and
            // raises OverflowError(inf)/ValueError(nan). Guard the rounded value
            // (finite + i64 range) and fail loud, mirroring the `int(float)`
            // guard (PMAT-586/589). The suffix arms below stay for match
            // exhaustiveness but are superseded by this guarded fast-path.
            if matches!(
                op,
                NumBuiltinOp::Floor | NumBuiltinOp::Ceil | NumBuiltinOp::Trunc
            ) {
                let round = match op {
                    NumBuiltinOp::Floor => "floor",
                    NumBuiltinOp::Ceil => "ceil",
                    _ => "trunc",
                };
                out.push_str("{ let __mf = (");
                emit_expr(out, &args[0], mode)?;
                write!(
                    out,
                    ").{round}(); if !__mf.is_finite() {{ panic!(\"xpile: math.{round}() of a non-finite float (Python OverflowError/ValueError)\"); }} if __mf < (i64::MIN as f64) || __mf >= (i64::MAX as f64) {{ panic!(\"xpile: math.{round}() out of i64 range; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); }} __mf as i64 }}"
                )?;
                return Ok(());
            }
            // PMAT-794 (HUNT-V18 EXC-003): Python raises `ValueError("math domain
            // error")` for `math.sqrt` of a negative and `math.log*` of a
            // non-positive; Rust's f64 `.sqrt()`/`.ln()`/`.log*()` return NaN/-inf
            // SILENTLY, so a guarding `except ValueError:` was dead code (no panic
            // ever fired). Guard the domain and panic with the tagged ValueError so
            // the allowlist `except` (PMAT-789) catches it, matching CPython.
            if matches!(
                op,
                NumBuiltinOp::Sqrt | NumBuiltinOp::Ln | NumBuiltinOp::Log10 | NumBuiltinOp::Log2
            ) {
                let (method, bad) = match op {
                    NumBuiltinOp::Sqrt => ("sqrt", "< 0.0"),
                    NumBuiltinOp::Ln => ("ln", "<= 0.0"),
                    NumBuiltinOp::Log10 => ("log10", "<= 0.0"),
                    _ => ("log2", "<= 0.0"),
                };
                out.push_str("{ let __ms = (");
                emit_expr(out, &args[0], mode)?;
                write!(
                    out,
                    "); if __ms {bad} {{ panic!(\"xpile: ValueError: math domain error\"); }} __ms.{method}() }}"
                )?;
                return Ok(());
            }
            out.push('(');
            emit_expr(out, &args[0], mode)?;
            out.push(')');
            match op {
                // PMAT-579: `abs` of an i64 must be checked — `i64::MIN.abs()`
                // wraps to `i64::MIN` silently (no overflow under `-O`), which
                // falsifies C-PY-INT-ARITH (Python's `abs` is exact). An f64
                // `abs` never overflows, so it keeps `.abs()`.
                NumBuiltinOp::Abs if *of_float => out.push_str(".abs()"),
                NumBuiltinOp::Abs => out.push_str(
                    ".checked_abs().expect(\"xpile: i64 abs overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")",
                ),
                // PMAT-502ek: math functions. `floor`/`ceil` return Python
                // `int`, so cast the f64 result to i64.
                NumBuiltinOp::Sqrt => out.push_str(".sqrt()"),
                NumBuiltinOp::Floor => out.push_str(".floor() as i64"),
                NumBuiltinOp::Ceil => out.push_str(".ceil() as i64"),
                // PMAT-502em: `math.trunc` — truncate toward zero, return int.
                NumBuiltinOp::Trunc => out.push_str(".trunc() as i64"),
                // PMAT-502el: trig / exp / log — 1-arg f64 → f64.
                NumBuiltinOp::Sin => out.push_str(".sin()"),
                NumBuiltinOp::Cos => out.push_str(".cos()"),
                NumBuiltinOp::Tan => out.push_str(".tan()"),
                NumBuiltinOp::Exp => out.push_str(".exp()"),
                NumBuiltinOp::Ln => out.push_str(".ln()"),
                NumBuiltinOp::Log10 => out.push_str(".log10()"),
                NumBuiltinOp::Log2 => out.push_str(".log2()"),
                NumBuiltinOp::Min | NumBuiltinOp::Max => {
                    // PMAT-502cz: variadic — chain `.min`/`.max` over every
                    // remaining arg (`max(a, b, c)` → `(a).max(b).max(c)`).
                    let method = if matches!(op, NumBuiltinOp::Min) {
                        ".min("
                    } else {
                        ".max("
                    };
                    for arg in &args[1..] {
                        out.push_str(method);
                        emit_expr(out, arg, mode)?;
                        out.push(')');
                    }
                }
            }
        }
        // PMAT-498b: `sum(xs)` → `<list>.iter().sum::<T>()`.
        Expr::Sum {
            list,
            of_float,
            start,
        } => {
            // PMAT-584: CPython's float `sum()` uses Neumaier compensated
            // summation (Py3.12+) — naive left-to-right `.iter().sum()` diverges
            // on catastrophic cancellation (`sum([1.0, 1e16, 1.0, -1e16])` is
            // 2.0, not 0.0; `sum([0.1]*10)` is 1.0, not 0.9999999999999999).
            // Emit the same compensated fold, seeded with `start` (or 0.0). The
            // int case stays exact `.iter().sum::<i64>()` (with `start`).
            if *of_float {
                out.push_str("{ let mut __ss: f64 = ");
                if let Some(start) = start {
                    out.push('(');
                    emit_expr(out, start, mode)?;
                    out.push(')');
                } else {
                    out.push_str("0.0f64");
                }
                out.push_str("; let mut __sc = 0.0f64; for &__sx in (");
                emit_expr(out, list, mode)?;
                // PMAT-679: skip compensation when the running total is
                // non-finite. Once `__st` is ±inf/NaN the Neumaier term computes
                // `inf - inf = NaN`, poisoning `__sc` and the result (Python
                // `sum([1.0, inf, 2.0])` is `inf`, not `NaN`). CPython resets the
                // compensation on a non-finite partial; reset `__sc = 0.0` so the
                // final `__ss + __sc` reflects the (non-finite) running total.
                out.push_str(").iter() { let __st = __ss + __sx; if __st.is_finite() { if __ss.abs() >= __sx.abs() { __sc += (__ss - __st) + __sx; } else { __sc += (__sx - __st) + __ss; } } else { __sc = 0.0f64; } __ss = __st; } __ss + __sc }");
            } else {
                // PMAT-595: integer `sum(xs[, start])` must honor the
                // C-PY-INT-ARITH overflow contract like every other int-arith
                // path (`+`, `*`, abs, the shift trio) — a bare
                // `.iter().sum::<i64>()` silently wraps under `-O` (and panics
                // with a generic message in debug), bypassing the contract.
                // Emit a checked fold seeded with `start` (or 0) that fails
                // loud, folding the start in so the seed is also contract-safe.
                out.push('(');
                emit_expr(out, list, mode)?;
                out.push_str(").iter().fold(");
                if let Some(start) = start {
                    out.push('(');
                    emit_expr(out, start, mode)?;
                    out.push(')');
                } else {
                    out.push_str("0i64");
                }
                out.push_str(", |__a, &__x| __a.checked_add(__x).expect(\"xpile: i64 addition overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"))");
            }
        }
        // PMAT-502j: `all(xs)`/`any(xs)` over a bool list.
        Expr::BoolReduce {
            list,
            is_all,
            short_circuit,
        } => {
            // PMAT-689: a short-circuiting (generator-expression) `any`/`all` over
            // a `Map` fuses the predicate into the `any`/`all` closure — Rust's
            // `any`/`all` short-circuit, matching Python's lazy genexpr, so a
            // not-yet-needed element is never evaluated (the prior eager
            // `.map(P).collect().iter().any(..)` panicked on e.g. a div-by-zero
            // element Python never reaches). The eager (list-comp / plain-list)
            // form keeps `.iter().any(|&__b| __b)`.
            if let (
                true,
                Expr::Map {
                    list: inner,
                    lambda,
                },
            ) = (*short_circuit, &**list)
            {
                emit_expr(out, inner, mode)?;
                let method = if *is_all { "all" } else { "any" };
                write!(
                    out,
                    ".iter().cloned().{method}(|__k| {{ let {} = __k.clone(); ",
                    lambda.param
                )?;
                emit_expr(out, &lambda.body, mode)?;
                out.push_str(" })");
            } else {
                emit_expr(out, list, mode)?;
                out.push_str(if *is_all {
                    ".iter().all(|&__b| __b)"
                } else {
                    ".iter().any(|&__b| __b)"
                });
            }
        }
        // PMAT-502k: `seq * n` → `(seq).repeat(((n).max(0)) as usize)`
        // (str → String, slice → Vec; negative count clamps to empty).
        Expr::Repeat { seq, n, of_str } => {
            if *of_str {
                // `String::repeat` — no `Copy` bound, unchanged.
                out.push('(');
                emit_expr(out, seq, mode)?;
                out.push_str(").repeat(((");
                emit_expr(out, n, mode)?;
                out.push_str(").max(0)) as usize)");
            } else {
                // PMAT-569: a list repeat clones its elements (slice `repeat`
                // needs `T: Copy`, which fails for `[[0]] * n` etc.). Works for
                // any `Clone` element; `.max(0)` clamps a negative count.
                out.push_str("{ let __rep = ");
                emit_expr(out, seq, mode)?;
                out.push_str("; (0..(((");
                emit_expr(out, n, mode)?;
                out.push_str(").max(0)) as usize)).flat_map(|_| __rep.iter().cloned()).collect::<Vec<_>>() }");
            }
        }
        // PMAT-502m: `int(x)`/`float(x)` → `((x) as i64)` / `((x) as f64)`.
        Expr::NumCast {
            value,
            to_float,
            from_str,
            from_float,
        } => {
            if *from_str && *to_float {
                // PMAT-502bf: `float(s)` → trimmed `.parse()` (panics on bad
                // input, matching Python's `ValueError`).
                // PMAT-611: Python `float()` also accepts PEP 515 underscores
                // BETWEEN digits (`float("1_000.5")` == 1000.5), which Rust's
                // `parse::<f64>()` rejects → panic. Validate that every `_` has an
                // ASCII digit on both sides (the exact Python rule, covering the
                // fractional/exponent parts), then strip + parse; invalid
                // placements (`1_.5`, `1.5_`, `1_e5`, `_1.0`) still raise.
                // Bind a *reference* (not the value) so a temporary-String operand
                // (`float("inf")`) survives the block via temporary lifetime
                // extension, and a reused variable operand is not moved (E0716).
                out.push_str("{ let __pf = &(");
                emit_expr(out, value, mode)?;
                out.push_str("); let __ps = __pf.trim(); let __pe = __ps.as_bytes(); if !__ps.bytes().enumerate().all(|(__k, __c)| __c != b'_' || (__k > 0 && __pe[__k - 1].is_ascii_digit() && __k + 1 < __pe.len() && __pe[__k + 1].is_ascii_digit())) { panic!(\"xpile: ValueError: could not convert string to float\"); } __ps.replace('_', \"\").parse::<f64>().expect(\"xpile: ValueError: could not convert string to float\") }");
            } else if *from_str {
                // PMAT-610: `int(s)` accepts PEP 515 underscore digit separators
                // (`int(\"1_000\") == 1000`), which Rust's `parse::<i64>()` rejects
                // → panic. Python allows a single `_` only BETWEEN digits, so for
                // an int (digits-only body after an optional sign) that is exactly
                // "no leading/trailing/doubled underscore"; validate that, then
                // strip the separators and parse. Invalid placements (or any
                // other bad literal) still panic ≈ Python `ValueError`.
                // Bind a *reference* (not the value) so a temporary-String operand
                // (`int("1_000")`) survives the block via temporary lifetime
                // extension, and a reused variable operand is not moved (E0716).
                out.push_str("{ let __pf = &(");
                emit_expr(out, value, mode)?;
                out.push_str("); let __ps = __pf.trim(); let __pb = __ps.strip_prefix('-').or_else(|| __ps.strip_prefix('+')).unwrap_or(__ps); if __pb.starts_with('_') || __pb.ends_with('_') || __pb.contains(\"__\") { panic!(\"xpile: ValueError: invalid literal for int()\"); } __ps.replace('_', \"\").parse::<i64>().expect(\"xpile: ValueError: invalid literal for int()\") }");
            } else if !*to_float && *from_float {
                // PMAT-586/589: `int(float_x)` — Python raises `OverflowError`
                // for `int(inf)` and `ValueError` for `int(nan)`, and returns an
                // exact (arbitrary-precision) integer for an out-of-i64-range
                // finite float like `int(1e30)`; Rust's `as i64` saturates
                // (`inf`/huge → `i64::MAX`) / zeroes (`nan` → 0) silently. Guard
                // both a non-finite source and an out-of-i64-range one and panic
                // (the contract's fail-loud posture until bigint promotion lands).
                out.push_str("{ let __ic = ");
                emit_expr(out, value, mode)?;
                // PMAT-793 (HUNT-V18 EXC-002): tag the non-finite panics with the
                // exact Python exception (`int(nan)` → ValueError, `int(±inf)` →
                // OverflowError) so the allowlist `except` (PMAT-789) discriminates
                // them — the old combined `xpile: int() of a non-finite float`
                // prefix matched no handler, so it merely propagated (and was
                // uncatchable by the right `except OverflowError`/`ValueError`).
                out.push_str("; if __ic.is_nan() { panic!(\"xpile: ValueError: cannot convert float NaN to integer\"); } if __ic.is_infinite() { panic!(\"xpile: OverflowError: cannot convert float infinity to integer\"); } if __ic < (i64::MIN as f64) || __ic >= (i64::MAX as f64) { panic!(\"xpile: int() out of i64 range; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); } __ic as i64 }");
            } else {
                out.push_str("((");
                emit_expr(out, value, mode)?;
                out.push_str(if *to_float { ") as f64)" } else { ") as i64)" });
            }
        }
        // PMAT-502ad/af: `str(x)` → `format!("{}", x)` (int) or a
        // Python-matching format block (float: `nan` + `".0"` whole-number suffix).
        Expr::ToStr { value, of_float } => {
            if *of_float {
                // PMAT-583: CPython float repr — scientific for decimal exponent
                // `< -4` or `>= 16`, else fixed (`.0`-if-whole). PMAT-842: the
                // block is shared with the dataclass-Display float-field path via
                // `py_float_repr_block` (single source — was duplicated).
                let mut v = String::new();
                emit_expr(&mut v, value, mode)?;
                out.push_str(&py_float_repr_block(&v));
            } else {
                out.push_str("format!(\"{}\", ");
                emit_expr(out, value, mode)?;
                out.push(')');
            }
        }
        // PMAT-582/778: `repr(str)` — CPython-style quoted form (single quote, or
        // double if the string has a `'` but no `"`), with `\\`/quote/`\n`/`\r`/
        // `\t` + control-char `\xNN` escaping. PMAT-842: the block is shared with
        // the dataclass-Display str-field path via `py_str_repr_block`.
        Expr::ReprStr { value } => {
            let mut v = String::new();
            emit_expr(&mut v, value, mode)?;
            out.push_str(&py_str_repr_block(&v));
        }
        // PMAT-502ak: `round(x)` (float) → `((x).round_ties_even() as i64)`
        // — banker's rounding, matching Python's `round`.
        Expr::RoundToInt { value } => {
            // PMAT-664: Python `round(x)` raises OverflowError on inf and
            // ValueError on nan, and returns an arbitrary-precision int for a
            // huge magnitude; a bare `as i64` saturated/garbage-cast silently.
            // Guard finiteness + i64 range (mirrors the int()/math.floor guards,
            // PMAT-586/589); out-of-range fails loud pending the bigint slow path.
            out.push_str("{ let __rti = (");
            emit_expr(out, value, mode)?;
            out.push_str(").round_ties_even(); if !__rti.is_finite() { panic!(\"xpile: round() of a non-finite float (Python OverflowError/ValueError)\"); } if __rti < (i64::MIN as f64) || __rti >= (i64::MAX as f64) { panic!(\"xpile: round() out of i64 range; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); } __rti as i64 }");
        }
        // PMAT-502al: `round(x, n)` (float) → Python's decimal rounding. For
        // n >= 0, format to n decimals (Rust's `{:.}` is round-half-to-even,
        // matching Python) and parse back; for n < 0, scale + round_ties_even.
        // PMAT-870 (HUNT-V31 #9): for n <= -309, `10f64.powi(-n)` overflows to
        // +inf, so `(x / inf).round() * inf` is `0.0 * inf` = NaN. Python rounds
        // to the nearest 10^|n| (== 0 for huge |n|), so guard the overflow and
        // return a sign-preserving zero (`__rx * 0.0` keeps -0.0 for negative x).
        Expr::RoundToDigits { value, ndigits } => {
            out.push_str("{ let __rx = ");
            emit_expr(out, value, mode)?;
            out.push_str("; let __rn = ");
            emit_expr(out, ndigits, mode)?;
            out.push_str("; if __rn >= 0 { format!(\"{:.1$}\", __rx, __rn as usize).parse::<f64>().unwrap() } else { let __rp = 10f64.powi((-__rn) as i32); if __rp.is_infinite() { __rx * 0.0 } else { (__rx / __rp).round_ties_even() * __rp } } }");
        }
        // PMAT-612: `round(int, n)` → int. For n >= 0 the int is returned
        // unchanged; for n < 0 it is rounded to the nearest multiple of
        // `10^(-n)` using round-half-to-EVEN (banker's rounding, like Python).
        // The arithmetic is done in `i128` so the scale and products can't
        // overflow; the result fails loud if it leaves `i64` range.
        Expr::RoundIntToDigits { value, ndigits } => {
            out.push_str("{ let __rv = (");
            emit_expr(out, value, mode)?;
            out.push_str(") as i128; let __rn = (");
            emit_expr(out, ndigits, mode)?;
            out.push_str("); if __rn >= 0 { __rv as i64 } else { let __rp = 10i128.checked_pow((-__rn) as u32).expect(\"xpile: OverflowError: round() scale out of range\"); let __rd = __rv.div_euclid(__rp); let __rm = __rv.rem_euclid(__rp); let __r2 = 2i128 * __rm; let __res = if __r2 < __rp { __rd * __rp } else if __r2 > __rp { (__rd + 1) * __rp } else if __rd % 2 == 0 { __rd * __rp } else { (__rd + 1) * __rp }; if __res < (i64::MIN as i128) || __res > (i64::MAX as i128) { panic!(\"xpile: OverflowError: round() result out of i64 range\"); } __res as i64 } }");
        }
        // PMAT-745 (HUNT-V13): exact int↔float comparison (`int OP float`).
        Expr::MixedIntFloatCmp { int, float, op } => {
            emit_mixed_int_float_cmp(out, int, float, *op, mode)?
        }
        // PMAT-502e: 1-arg `min(xs)`/`max(xs)` reduction over an int list.
        // PMAT-502h: `list[float]` uses a fold (f64 has no `Ord`).
        // PMAT-502aa: `key=lambda p: e` → `min_by_key`/`max_by_key`.
        Expr::ListMinMax {
            list,
            is_max,
            of_float,
            of_struct_cmp,
            key,
            default,
        } => {
            // PMAT-502dh: a `default` makes the empty case return it (via
            // `.unwrap_or(<default>)`) instead of panicking; the float branch
            // switches from the ±∞ fold to a `.reduce(..).unwrap_or(<default>)`.
            emit_expr(out, list, mode)?;
            match key {
                // PMAT-653: a FLOAT-returning key makes the compared values `f64`
                // (no `Ord`), so `max_by_key`/`min_by_key` is E0277. Compare
                // recomputed keys with `partial_cmp` (mirrors the Sorted float-key
                // path, PMAT-603). `max` reverses first so ties resolve to the
                // FIRST element (Python semantics, PMAT-568); a NaN key falls back
                // to `Equal` (Python's max/min don't raise on NaN keys, PMAT-616).
                Some(k) if *of_float => {
                    if *is_max {
                        write!(
                            out,
                            ".iter().cloned().rev().max_by(|__a, __b| {{ let {p} = __a.clone(); ",
                            p = k.param
                        )?;
                    } else {
                        write!(
                            out,
                            ".iter().cloned().min_by(|__a, __b| {{ let {p} = __a.clone(); ",
                            p = k.param
                        )?;
                    }
                    emit_expr(out, &k.body, mode)?;
                    write!(
                        out,
                        " }}.partial_cmp(&{{ let {p} = __b.clone(); ",
                        p = k.param
                    )?;
                    emit_expr(out, &k.body, mode)?;
                    out.push_str(" }).unwrap_or(std::cmp::Ordering::Equal))");
                }
                Some(k) => {
                    // PMAT-568: Python `max(key=)` returns the FIRST element with
                    // the maximal key, but Rust's `max_by_key` returns the LAST.
                    // Reverse the iterator first so its last-wins picks the
                    // original first maximum. `min` is unaffected — both Python
                    // `min` and Rust `min_by_key` keep the first minimum.
                    if *is_max {
                        write!(
                            out,
                            ".iter().cloned().rev().max_by_key(|__k| {{ let {} = __k.clone(); ",
                            k.param
                        )?;
                    } else {
                        write!(
                            out,
                            ".iter().cloned().min_by_key(|__k| {{ let {} = __k.clone(); ",
                            k.param
                        )?;
                    }
                    emit_expr(out, &k.body, mode)?;
                    out.push_str(" })");
                }
                // PMAT-889 (HUNT-V33 #4): a struct element with a custom `__lt__`
                // is PartialOrd-but-not-Ord AND not Copy, so neither the `Ord`
                // `.max()` path nor the float `.copied().reduce(..)` path applies.
                // Use `.cloned().max_by(partial_cmp)`; `max` reverses first so
                // ties resolve to the FIRST element (Python semantics, like the
                // keyed-float path); a `None` partial_cmp falls back to `Equal`.
                None if *of_struct_cmp => {
                    if *is_max {
                        out.push_str(".iter().cloned().rev().max_by(|__a, __b| __a.partial_cmp(__b).unwrap_or(std::cmp::Ordering::Equal))");
                    } else {
                        out.push_str(".iter().cloned().min_by(|__a, __b| __a.partial_cmp(__b).unwrap_or(std::cmp::Ordering::Equal))");
                    }
                }
                None => match *of_float {
                    // Ord element (i64 / str / bool): `.min()/.max()` returns
                    // Option. `.cloned()` (not `.copied()`) so non-Copy
                    // `String` works too (PMAT-502er); i64/bool are `Clone`.
                    false => out.push_str(if *is_max {
                        ".iter().cloned().max()"
                    } else {
                        ".iter().cloned().min()"
                    }),
                    // PMAT-608: float min/max follow Python's first-argument-wins
                    // semantics (and NaN propagation), NOT `f64::max`/`f64::min`,
                    // via a strict-compare `reduce` (→ Option). This also fixes
                    // the empty case: the old `fold(±∞, …)` returned ±∞ for an
                    // empty sequence; `reduce` yields `None`, unwrapped below to
                    // a Python-`ValueError`-style panic (or the default).
                    true => {
                        let cmp = if *is_max { ">" } else { "<" };
                        write!(
                            out,
                            ".iter().copied().reduce(|__a, __b| if __b {cmp} __a {{ __b }} else {{ __a }})"
                        )?;
                    }
                },
            }
            // Every branch now yields an `Option`; unwrap (empty → Python
            // ValueError) or substitute the default.
            match default {
                Some(d) => {
                    out.push_str(".unwrap_or(");
                    emit_expr(out, d, mode)?;
                    out.push(')');
                }
                // PMAT-774 (HUNT-V16 CG-5): `max()`/`min()` over an empty sequence
                // (e.g. an empty filtered comprehension) raises Python
                // `ValueError: max() arg is an empty sequence`. The int/Ord branch
                // emitted a bare `.unwrap()` (native "Option::unwrap on None"
                // panic), and the float branch's message lacked the `xpile:
                // ValueError: ` prefix — so neither was caught by a typed `except
                // ValueError:` (the PMAT-731 re-raise filter matches that prefix).
                // Emit the canonical tagged message in BOTH branches.
                None => {
                    let fname = if *is_max { "max" } else { "min" };
                    write!(
                        out,
                        ".expect(\"xpile: ValueError: {fname}() arg is an empty sequence\")"
                    )?;
                }
            }
        }
        // PMAT-502u: list query — `xs.count(x)` / `xs.index(x)` (→ i64).
        Expr::ListQuery { list, op, arg } => {
            emit_expr(out, list, mode)?;
            match op {
                // PMAT-853: compare by place (`**__e == arg`) rather than
                // destructuring by copy (`|&&__e|`), so a non-Copy element type
                // (`String`, …) works as well as `i64`/`bool`.
                ListQueryOp::Count => {
                    out.push_str(".iter().filter(|__e| **__e == ");
                    emit_expr(out, arg, mode)?;
                    out.push_str(").count() as i64");
                }
                ListQueryOp::Index => {
                    // `position` yields `&T` (one ref) — single deref, unlike
                    // `filter`'s `&&T`.
                    out.push_str(".iter().position(|__e| *__e == ");
                    emit_expr(out, arg, mode)?;
                    out.push_str(").map(|__i| __i as i64).expect(\"xpile: ValueError: list.index(x): x not in list\")");
                }
            }
        }
        // PMAT-502as: `xs.pop()` → `(<list>).pop().unwrap()` (last; panics
        // if empty, matching Python IndexError); `xs.pop(i)` →
        // `(<list>).remove((<i>) as usize)` (panics if out of range).
        Expr::ListPop { list, index } => match index {
            None => {
                // PMAT-715: `xs[i].pop()` must mutate the element IN PLACE. The
                // read path (`emit_expr`) clones the inner container, so the pop
                // mutated a throwaway clone (silent-wrong — `xs[i]` kept its
                // length). When the receiver is `Ident[index]`, emit an l-value
                // place `base[norm(i)].pop().unwrap()` (the base is marked `mut`
                // by `count_pop_receivers`); a normalized negative index wraps like
                // Python. Deeper/other receivers keep the (read-clone) form.
                let lvalue_base = match list.as_ref() {
                    Expr::Index {
                        collection,
                        index: idx,
                    } => match collection.as_ref() {
                        Expr::Ident(base) => Some((base.clone(), idx.as_ref())),
                        _ => None,
                    },
                    _ => None,
                };
                if let Expr::DictGet { dict, key } = list.as_ref() {
                    // PMAT-797 (HUNT-V19 ND-01): `d[k].pop()` must mutate the
                    // stored list IN PLACE. The dict read (`emit_expr` of
                    // `DictGet`) clones the value (`.get(&k).cloned()`), so the pop
                    // hit a throwaway clone and the stored list kept its length
                    // (silent-wrong — `len(d[k])` unchanged). Reach the value
                    // mutably via `get_mut(&k)` instead (the dict base is marked
                    // `mut` by `count_pop_receivers`); a missing key still raises
                    // the tagged KeyError, an empty list the tagged IndexError.
                    out.push('(');
                    emit_expr(out, dict, mode)?;
                    out.push_str(").get_mut(&(");
                    emit_expr(out, key, mode)?;
                    out.push_str(")).unwrap_or_else(|| panic!(\"xpile: KeyError: key not found\")).pop().expect(\"xpile: IndexError: pop from empty list\")");
                } else if let Some((base, idx)) = lvalue_base {
                    out.push_str("{ let __pi = (");
                    emit_expr(out, idx, mode)?;
                    write!(
                        out,
                        ") as i64; let __pi = if __pi < 0 {{ {base}.len() as i64 + __pi }} else {{ __pi }}; {base}[__pi as usize].pop().expect(\"xpile: IndexError: pop from empty list\") }}"
                    )?;
                } else {
                    // PMAT-747 (HUNT-V14 #2): `xs.pop()` on an empty list raises
                    // Python IndexError — tag the panic (`xpile: IndexError:`) so
                    // a typed `except` discriminates it instead of swallowing the
                    // untagged native `Option::unwrap` panic.
                    out.push('(');
                    emit_expr(out, list, mode)?;
                    out.push_str(").pop().expect(\"xpile: IndexError: pop from empty list\")");
                }
            }
            // PMAT-570: a negative-resolved index (`len(xs) - k`) references the
            // receiver, conflicting with `remove`'s mutable borrow (E0502) — bind
            // it first. Positive indices keep the inline form.
            Some(i) => {
                let refs_self =
                    matches!(list.as_ref(), Expr::Ident(n) if expr_mentions_ident(i, n));
                if refs_self {
                    out.push_str("{ let __pi = (");
                    emit_expr(out, i, mode)?;
                    out.push_str(") as usize; (");
                    emit_expr(out, list, mode)?;
                    out.push_str(").remove(__pi) }");
                } else {
                    out.push('(');
                    emit_expr(out, list, mode)?;
                    out.push_str(").remove((");
                    emit_expr(out, i, mode)?;
                    out.push_str(") as usize)");
                }
            }
        },
        // PMAT-502au: `d.pop(k)` → `(<dict>).remove(&(<key>)).unwrap()`
        // (panics if absent, matching Python `KeyError`); `d.pop(k, def)`
        // → `(<dict>).remove(&(<key>)).unwrap_or(<default>)`.
        Expr::DictPop { dict, key, default } => {
            out.push('(');
            emit_expr(out, dict, mode)?;
            out.push_str(").shift_remove(&(");
            emit_expr(out, key, mode)?;
            match default {
                // PMAT-747 (HUNT-V14 #2): `d.pop(k)` on an absent key raises
                // Python KeyError — tag the panic so a typed `except`
                // discriminates it (the untagged native `unwrap` was swallowed).
                None => {
                    out.push_str(")).unwrap_or_else(|| panic!(\"xpile: KeyError: key not found\"))")
                }
                Some(d) => {
                    out.push_str(")).unwrap_or(");
                    emit_expr(out, d, mode)?;
                    out.push(')');
                }
            }
        }
        // PMAT-502ax: `d.setdefault(k, default)` →
        // `(<dict>).entry(<key>.clone()).or_insert(<default>).clone()`.
        // PMAT-843 (HUNT-V27 #1): bind the default into a temp BEFORE `.entry()`.
        // A default that READS the dict (`d.setdefault(k, d[x])` / `d.get(...)` /
        // `len(d)`) borrowed `d` immutably inside `or_insert(...)` while the
        // `entry()` mutable borrow was live → rustc E0502. Python evaluates the
        // default eagerly regardless (it's a plain argument), and `or_insert`
        // already takes it eagerly, so hoisting only moves the borrow earlier — no
        // behaviour change. (The key in `.entry(...)` is fine via two-phase
        // borrow.) Mirrors the nested-dict RMW RHS hoist (PMAT-833).
        Expr::DictSetDefault { dict, key, default } => {
            out.push_str("{ let __sd_def = ");
            emit_expr(out, default, mode)?;
            out.push_str("; (");
            emit_expr(out, dict, mode)?;
            out.push_str(").entry((");
            emit_expr(out, key, mode)?;
            out.push_str(").clone()).or_insert(__sd_def).clone() }");
        }
        // PMAT-502c/f/z: `sorted(xs)` → `{ let mut __xv = <list>.clone();
        // __xv.sort(); __xv }`; `reverse=True` appends `__xv.reverse();`;
        // `key=lambda p: e` uses `__xv.sort_by_key(|__k| { let p = __k.clone(); e })`.
        Expr::Sorted {
            list,
            reverse,
            key,
            of_float,
        } => {
            out.push_str("{ let mut __xv = ");
            emit_expr(out, list, mode)?;
            out.push_str(".clone(); __xv.");
            match (key, *reverse) {
                // PMAT-578: `Vec<f64>` has no `Ord`, so a keyless float sort uses
                // `sort_by(partial_cmp)`; an i64 list keeps `.sort()`. Mirrors
                // `ListMutateOp::Sort`.
                // PMAT-616: a NaN element makes `partial_cmp` return `None`;
                // Python's `sorted` does NOT raise on NaN, so fall back to `Equal`
                // rather than `.unwrap()` panicking (identical for finite floats).
                (None, false) if *of_float => {
                    out.push_str(
                        "sort_by(|__a, __b| __a.partial_cmp(__b).unwrap_or(std::cmp::Ordering::Equal));",
                    );
                }
                (None, true) if *of_float => {
                    out.push_str(
                        "sort_by(|__a, __b| __b.partial_cmp(__a).unwrap_or(std::cmp::Ordering::Equal));",
                    );
                }
                (None, false) => out.push_str("sort();"),
                // Equal elements are identical, so reverse() can't disturb order.
                (None, true) => out.push_str("sort(); __xv.reverse();"),
                // PMAT-603: a FLOAT-returning key makes the comparison values
                // `f64` (no `Ord`) — `sort_by_key` is E0277. Compare the
                // recomputed key with `partial_cmp`.
                // PMAT-616: a NaN key falls back to `Equal` (Python doesn't raise).
                (Some(k), false) if *of_float => {
                    write!(
                        out,
                        "sort_by(|__a, __b| {{ let {p} = __a.clone(); ",
                        p = k.param
                    )?;
                    emit_expr(out, &k.body, mode)?;
                    write!(
                        out,
                        " }}.partial_cmp(&{{ let {p} = __b.clone(); ",
                        p = k.param
                    )?;
                    emit_expr(out, &k.body, mode)?;
                    out.push_str(" }).unwrap_or(std::cmp::Ordering::Equal));");
                }
                (Some(k), false) => {
                    write!(out, "sort_by_key(|__k| {{ let {} = __k.clone(); ", k.param)?;
                    emit_expr(out, &k.body, mode)?;
                    out.push_str(" });");
                }
                // PMAT-568: Python `sorted(key=, reverse=True)` is STABLE — equal-
                // key elements keep their ORIGINAL order. `sort_by_key` + `.reverse()`
                // flips them (descending-stable, not original-order-preserving); use
                // a stable descending comparator on the key instead.
                (Some(k), true) => {
                    write!(
                        out,
                        "sort_by(|__a, __b| {{ let __ka = {{ let {p} = __a.clone(); ",
                        p = k.param
                    )?;
                    emit_expr(out, &k.body, mode)?;
                    write!(
                        out,
                        " }}; let __kb = {{ let {p} = __b.clone(); ",
                        p = k.param
                    )?;
                    emit_expr(out, &k.body, mode)?;
                    // PMAT-603: a float key compares with `partial_cmp` (no `Ord`);
                    // integer/str keys use `cmp`. Descending + stable either way.
                    if *of_float {
                        // PMAT-616: NaN key → `Equal` (Python doesn't raise on NaN).
                        out.push_str(
                            " }; __kb.partial_cmp(&__ka).unwrap_or(std::cmp::Ordering::Equal) });",
                        );
                    } else {
                        out.push_str(" }; __kb.cmp(&__ka) });");
                    }
                }
            }
            out.push_str(" __xv }");
        }
        // PMAT-502d: `reversed(xs)` → a new reversed Vec.
        Expr::Reversed { list } => {
            out.push_str("{ let mut __xv = ");
            emit_expr(out, list, mode)?;
            out.push_str(".clone(); __xv.reverse(); __xv }");
        }
        // PMAT-549: `math.gcd(a, b)` → inline Euclidean algorithm over abs values.
        Expr::Gcd { a, b } => {
            out.push_str("{ let mut __ga = (");
            emit_expr(out, a, mode)?;
            out.push_str(").abs(); let mut __gb = (");
            emit_expr(out, b, mode)?;
            out.push_str(").abs(); while __gb != 0 { let __gt = __gb; __gb = __ga % __gb; __ga = __gt; } __ga }");
        }
        // PMAT-550: `math.lcm(a, b)` → `(abs(a)/gcd) * abs(b)` (0 if either is 0).
        Expr::Lcm { a, b } => {
            out.push_str("{ let __la = (");
            emit_expr(out, a, mode)?;
            out.push_str(").abs(); let __lb = (");
            emit_expr(out, b, mode)?;
            out.push_str(").abs(); if __la == 0 || __lb == 0 { 0 } else { let mut __ga = __la; let mut __gb = __lb; while __gb != 0 { let __gt = __gb; __gb = __ga % __gb; __ga = __gt; } (__la / __ga) * __lb } }");
        }
        // PMAT-551: `math.factorial(n)` → inline product loop (checked, n>=0).
        Expr::Factorial { n } => {
            out.push_str("{ let __nf = (");
            emit_expr(out, n, mode)?;
            out.push_str("); if __nf < 0 { panic!(\"xpile: ValueError: factorial() not defined for negative values\"); } let mut __f = 1i64; let mut __fi = 2i64; while __fi <= __nf { __f = __f.checked_mul(__fi).expect(\"xpile: i64 multiplication overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); __fi += 1; } __f }");
        }
        // PMAT-552: `math.isqrt(n)` → exact integer Newton (no float).
        Expr::Isqrt { n } => {
            out.push_str("{ let __sn = (");
            emit_expr(out, n, mode)?;
            out.push_str("); if __sn < 0 { panic!(\"xpile: ValueError: isqrt() argument must be nonnegative\"); } if __sn == 0 { 0 } else { let mut __sx = 1i64 << ((64 - __sn.leading_zeros() + 1) / 2); loop { let __sy = (__sx + __sn / __sx) / 2; if __sy >= __sx { break; } __sx = __sy; } __sx } }");
        }
        // PMAT-553: `math.comb(n, k)` → incremental binomial product (k>n → 0).
        Expr::Comb { n, k } => {
            out.push_str("{ let __cn = (");
            emit_expr(out, n, mode)?;
            out.push_str("); let __ck = (");
            emit_expr(out, k, mode)?;
            out.push_str("); if __cn < 0 || __ck < 0 { panic!(\"xpile: ValueError: comb() arguments must be non-negative\"); } if __ck > __cn { 0 } else { let __ck2 = if __ck < __cn - __ck { __ck } else { __cn - __ck }; let mut __cr = 1i64; let mut __ci = 0i64; while __ci < __ck2 { __cr = __cr.checked_mul(__cn - __ci).expect(\"xpile: i64 multiplication overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\") / (__ci + 1); __ci += 1; } __cr } }");
        }
        // PMAT-554: `math.perm(n, k)` → descending product of k factors (k>n → 0).
        Expr::Perm { n, k } => {
            out.push_str("{ let __pn = (");
            emit_expr(out, n, mode)?;
            out.push_str("); let __pk = (");
            emit_expr(out, k, mode)?;
            out.push_str("); if __pn < 0 || __pk < 0 { panic!(\"xpile: ValueError: perm() arguments must be non-negative\"); } if __pk > __pn { 0 } else { let mut __pr = 1i64; let mut __pi = 0i64; while __pi < __pk { __pr = __pr.checked_mul(__pn - __pi).expect(\"xpile: i64 multiplication overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); __pi += 1; } __pr } }");
        }
        // PMAT-571: `pow(base, exp, mod)` → modular exponentiation (square &
        // multiply, reduced mod m each step via i128 products → no overflow).
        // PMAT-605: Python's 3-arg `pow` returns a result with the SIGN of the
        // modulus (range `(m, 0]` for `m < 0`); the square-multiply loop yields
        // the non-negative Euclidean residue, so re-sign at the end when `m < 0`.
        Expr::PowMod { base, exp, modulus } => {
            out.push_str("{ let __pmm = (");
            emit_expr(out, modulus, mode)?;
            out.push_str("); if __pmm == 0 { panic!(\"xpile: ValueError: pow() 3rd argument cannot be 0\"); } let __pme = (");
            emit_expr(out, exp, mode)?;
            out.push_str("); if __pme < 0 { panic!(\"xpile: ValueError: pow() 2nd argument cannot be negative when 3rd argument specified\"); } let __pmb0 = (");
            emit_expr(out, base, mode)?;
            // PMAT-619: do the whole modexp on the MAGNITUDE `|m|` (in i128, so
            // `|i64::MIN|` doesn't overflow), then sign-correct the residue to the
            // modulus's sign at the end. The old base-normalization
            // `if __t < 0 { __t + __pmm }` and the `% __pmm` reductions assumed a
            // POSITIVE modulus, so a NEGATIVE modulus (esp. with a negative base)
            // produced wrong values (`pow(-2,3,-5)` gave 3, Python gives -3).
            // Python `pow(a,b,m)` with `m<0` returns the residue in `(m, 0]`.
            out.push_str("); let __pma = (__pmm as i128).abs(); let mut __pmb = { let __t = (__pmb0 as i128) % __pma; if __t < 0 { __t + __pma } else { __t } }; let mut __pmr = 1i128 % __pma; let mut __pmk = __pme; while __pmk > 0 { if __pmk & 1 == 1 { __pmr = (__pmr * __pmb) % __pma; } __pmk >>= 1; __pmb = (__pmb * __pmb) % __pma; } if __pmm < 0 && __pmr != 0 { __pmr -= __pma; } __pmr as i64 }");
        }
        // PMAT-502cj: `list(range(start, stop, step))` → a collected i64 range.
        Expr::RangeList { start, stop, step } => {
            if *step > 0 {
                out.push('(');
                emit_expr(out, start, mode)?;
                out.push_str("..");
                emit_expr(out, stop, mode)?;
                out.push(')');
                if *step != 1 {
                    write!(out, ".step_by({step}usize)")?;
                }
            } else {
                // PMAT-523: negative-step range — Python `range(start, stop,
                // step<0)` = `((stop)+1 ..= (start)).rev().step_by(|step|)`.
                out.push_str("(((");
                emit_expr(out, stop, mode)?;
                out.push_str(") + 1)..=(");
                emit_expr(out, start, mode)?;
                out.push_str(")).rev()");
                let abs = -*step;
                if abs != 1 {
                    write!(out, ".step_by({abs}usize)")?;
                }
            }
            out.push_str(".collect::<Vec<i64>>()");
        }
        // PMAT-502cw: `set(xs)` → collect the list into a HashSet.
        Expr::SetFromList { list } => {
            emit_expr(out, list, mode)?;
            out.push_str(".iter().cloned().collect::<std::collections::HashSet<_>>()");
        }
        // PMAT-520: `list(<set>)` / `sorted(<set>)` → the set's unique elements
        // as a Vec.
        Expr::SetToList { set } => {
            emit_expr(out, set, mode)?;
            out.push_str(".iter().cloned().collect::<Vec<_>>()");
        }
        // PMAT-502dk: `dict(pairs)` → a HashMap from the list of 2-tuples.
        Expr::DictFromPairs { pairs } => {
            emit_expr(out, pairs, mode)?;
            out.push_str(".iter().cloned().collect::<indexmap::IndexMap<_, _>>()");
        }
        // PMAT-502dw/dx: `{k: v, **d, …}` → chain each fragment's iterator
        // (explicit pair → `once((k, v))`; splat → `(d).iter().map(clone)`)
        // into a fresh HashMap (a later entry wins, matching Python).
        Expr::DictMerge { entries } => {
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(".chain(");
                }
                match k {
                    Some(key) => {
                        out.push_str("std::iter::once((");
                        emit_expr(out, key, mode)?;
                        out.push_str(", ");
                        emit_expr(out, v, mode)?;
                        out.push_str("))");
                    }
                    None => {
                        out.push('(');
                        emit_expr(out, v, mode)?;
                        out.push_str(").iter().map(|(__k, __v)| (__k.clone(), __v.clone()))");
                    }
                }
                if i > 0 {
                    out.push(')');
                }
            }
            out.push_str(".collect::<indexmap::IndexMap<_, _>>()");
        }
        // PMAT-502ab: `filter(pred, xs)` → `.iter().cloned().filter(|__k| {
        // let p = __k.clone(); pred }).collect::<Vec<_>>()`.
        Expr::Filter { list, lambda } => {
            emit_expr(out, list, mode)?;
            write!(
                out,
                ".iter().cloned().filter(|__k| {{ let {} = __k.clone(); ",
                lambda.param
            )?;
            emit_expr(out, &lambda.body, mode)?;
            out.push_str(" }).collect::<Vec<_>>()");
        }
        // PMAT-502ac: `map(f, xs)` → `.iter().cloned().map(|__k| { let p =
        // __k.clone(); e }).collect::<Vec<_>>()`.
        Expr::Map { list, lambda } => {
            emit_expr(out, list, mode)?;
            write!(
                out,
                ".iter().cloned().map(|__k| {{ let {} = __k.clone(); ",
                lambda.param
            )?;
            emit_expr(out, &lambda.body, mode)?;
            out.push_str(" }).collect::<Vec<_>>()");
        }
        // PMAT-502ai: `enumerate(xs)` → Vec of (i64, elem) tuples.
        Expr::Enumerate { list, start } => {
            emit_expr(out, list, mode)?;
            // PMAT-684: `start` offsets the index. `start == 0` is the bare form;
            // a non-zero start adds via `checked_add` (honoring C-PY-INT-ARITH),
            // mirroring the for-loop `PairIterKind::Enumerate { start }` (PMAT-595).
            if *start == 0 {
                out.push_str(
                    ".iter().cloned().enumerate().map(|(__i, __e)| (__i as i64, __e)).collect::<Vec<_>>()",
                );
            } else {
                write!(
                    out,
                    ".iter().cloned().enumerate().map(|(__i, __e)| ((__i as i64).checked_add({start}i64).expect(\"xpile: i64 addition overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"), __e)).collect::<Vec<_>>()"
                )?;
            }
        }
        // PMAT-502ai: `zip(xs, ys)` → Vec of paired tuples.
        Expr::Zip { left, right } => {
            emit_expr(out, left, mode)?;
            out.push_str(".iter().cloned().zip(");
            emit_expr(out, right, mode)?;
            out.push_str(".iter().cloned()).collect::<Vec<_>>()");
        }
        // PMAT-462 (v0.2.0 Track 1.C): Python dict literal →
        // Rust `{ let mut m = HashMap::new(); m.insert(k, v); ... m }`
        // block expression returning the owned HashMap.
        Expr::DictLit(pairs) => {
            // PMAT-466: the empty literal emits a bare `HashMap::new()`
            // (the surrounding `let`'s annotation supplies K/V). A
            // `{ let mut __xpile_map = …; __xpile_map }` block with no inserts
            // would trip clippy's `unused_mut` under `-D warnings`.
            if pairs.is_empty() {
                out.push_str("indexmap::IndexMap::new()");
            } else {
                // PMAT-720 (HUNT-V8 V8-EXTRA): the accumulator is named
                // `__xpile_map`, not `m` — a user variable named `m` (`{m: 1}`)
                // would otherwise be shadowed by the inner `let mut m`, so the
                // bare-ident key/value `m.clone()` referenced the HashMap, not the
                // user's `m` (inserting the map into itself → E0275 / a wrong key).
                // The `__xpile_*` prefix is xpile's reserved temp namespace.
                out.push_str("{ let mut __xpile_map = indexmap::IndexMap::new(); ");
                for (k, v) in pairs {
                    // PMAT-699: a bare-variable (`Expr::Ident`) key or value is
                    // MOVED into `__xpile_map.insert(...)`; reusing it afterward
                    // (`d[k]`, `len(s)`, another insert of the same name) was E0382.
                    // Clone bare idents at the insert (mirrors the `DictSet`/dict-comp
                    // key clone). Literals and temporaries (calls, arithmetic)
                    // produce fresh values and are emitted as-is — no redundant
                    // clone. (Clone-on-Copy is clippy-only; generated code is
                    // compiled with `rustc -A warnings`.)
                    out.push_str("__xpile_map.insert(");
                    emit_expr(out, k, mode)?;
                    if matches!(k, Expr::Ident(_)) {
                        out.push_str(".clone()");
                    }
                    out.push_str(", ");
                    emit_expr(out, v, mode)?;
                    if matches!(v, Expr::Ident(_)) {
                        out.push_str(".clone()");
                    }
                    out.push_str("); ");
                }
                out.push_str("__xpile_map }");
            }
        }
        // PMAT-457 (v0.2.0 Track 1.B): Python `xs[i]` → Rust
        // `xs[i as usize].clone()`. The `.clone()` produces an owned value
        // matching the v0.2.0 owned-only ownership posture.
        // PMAT-639: a runtime-negative index must wrap like Python (`i < 0 →
        // len + i`), mirroring the str-index path — emitting `xs[i as usize]`
        // for a variable index panicked (`-1 as usize` = usize::MAX) where
        // Python returns the last element. A NON-NEGATIVE integer LITERAL index
        // keeps the bare fast path (no wrap needed, no churn); a literal
        // NEGATIVE index is already resolved to `len - k` in the frontend, so it
        // never reaches here negative. (`Expr::Index` is list-only — str has its
        // own char-indexed path, dict uses `DictGet`.)
        Expr::Index { collection, index } => {
            let nonneg_literal = matches!(index.as_ref(), Expr::LitInt(n) if *n >= 0);
            if nonneg_literal {
                // PMAT-764 (HUNT-V16 #4): even a non-negative LITERAL index needs
                // an `xpile: IndexError:`-tagged bounds check. A bare `coll[N]`
                // panics with Rust's NATIVE "index out of bounds" message, which
                // carries no `xpile:` prefix — so an OOB literal index inside a
                // `try` was SILENTLY SWALLOWED by an unrelated typed `except`
                // (e.g. `except KeyError:` caught it), where Python propagates the
                // IndexError. Bind + bounds-check with the tag (mirrors the
                // runtime/negative path, PMAT-744). `&(...)` also handles a
                // block-producing collection (sorted/reversed). LLVM elides the
                // redundant check for an in-range literal under -O.
                out.push_str("{ let __lc = &(");
                emit_expr(out, collection, mode)?;
                out.push_str("); let __li = (");
                emit_expr(out, index, mode)?;
                out.push_str(") as usize; if __li >= __lc.len() { panic!(\"xpile: IndexError: list index out of range\"); } __lc[__li].clone() }");
            } else {
                // PMAT-639: bind the collection (by ref, eval-once — `&(...)`
                // also handles a block-producing collection) and the index,
                // then wrap a negative index.
                // PMAT-744 (HUNT-V13 exc-flow-01/02): emit an explicit bounds
                // check that panics with the `xpile: IndexError:` TAG, instead of
                // relying on the native `Vec` `[i]` panic (whose message carries
                // no `xpile:` prefix). The typed-`except` discrimination
                // (PMAT-731) only re-raises panics tagged `xpile: <KnownExc>:`, so
                // an untagged native bounds panic was being SILENTLY SWALLOWED by
                // an unrelated `except ValueError:` (etc.) — Python propagates the
                // IndexError. Tagging makes `except IndexError` catch it and every
                // other typed `except` correctly re-raise it.
                out.push_str("{ let __lc = &(");
                emit_expr(out, collection, mode)?;
                out.push_str("); let __li: i64 = (");
                emit_expr(out, index, mode)?;
                out.push_str(") as i64; let __lidx = if __li < 0 { __lc.len() as i64 + __li } else { __li }; if __lidx < 0 || __lidx as usize >= __lc.len() { panic!(\"xpile: IndexError: list index out of range\"); } __lc[__lidx as usize].clone() }");
            }
        }
        // PMAT-466 (v0.2.0 Track 1.C): Python `d[k]` → Rust dict-index read.
        // PMAT-747 (HUNT-V14 #2): an absent key must panic with the
        // `xpile: KeyError:` TAG, not HashMap's native `Index` panic ("no entry
        // found for key") — the typed-`except` re-raise filter (PMAT-731) only
        // re-raises panics tagged `xpile: <KnownExc>:`, so an untagged native
        // KeyError was being SILENTLY SWALLOWED by an unrelated `except
        // ValueError:` (Python propagates the KeyError). Emit `.get(&k).cloned()`
        // + a tagged `unwrap_or_else` panic so `except KeyError` catches it and
        // every other typed `except` re-raises it. `.cloned()` keeps the owned
        // value (v0.2.0 owned-only posture).
        Expr::DictGet { dict, key } => {
            out.push('(');
            emit_expr(out, dict, mode)?;
            out.push_str(").get(&(");
            emit_expr(out, key, mode)?;
            out.push_str(
                ")).cloned().unwrap_or_else(|| panic!(\"xpile: KeyError: key not found\"))",
            );
        }
        // PMAT-466: Python `d.get(k, default)` → Rust
        // `d.get(&(k)).cloned().unwrap_or(default)`. Total: never
        // panics; returns `default` for an absent key.
        Expr::DictGetOr { dict, key, default } => {
            emit_expr(out, dict, mode)?;
            out.push_str(".get(&(");
            emit_expr(out, key, mode)?;
            out.push_str(")).cloned().unwrap_or(");
            emit_expr(out, default, mode)?;
            out.push(')');
        }
        // PMAT-502ey: 1-arg `d.get(k)` → `(d).get(&(k)).cloned()` : Option<V>.
        Expr::DictGetOpt { dict, key } => {
            emit_expr(out, dict, mode)?;
            out.push_str(".get(&(");
            emit_expr(out, key, mode)?;
            out.push_str(")).cloned()");
        }
        // PMAT-466: Python `k in d` → Rust `d.contains_key(&(k))`.
        Expr::DictContains { dict, key } => {
            emit_expr(out, dict, mode)?;
            out.push_str(".contains_key(&(");
            emit_expr(out, key, mode)?;
            out.push_str("))");
        }
        // PMAT-502v/502x: `d.keys()`/`d.values()`/`d.items()` → materialized Vec.
        Expr::DictView { dict, kind } => {
            emit_expr(out, dict, mode)?;
            out.push_str(match kind {
                DictViewKind::Keys => ".keys().cloned().collect::<Vec<_>>()",
                DictViewKind::Values => ".values().cloned().collect::<Vec<_>>()",
                DictViewKind::Items => {
                    ".iter().map(|(__k, __v)| (__k.clone(), __v.clone())).collect::<Vec<_>>()"
                }
            });
        }
        // PMAT-500: Python set literal `{a, b, c}` → HashSet-init block.
        // PMAT-501b: an empty SetLit (the set-comprehension accumulator)
        // emits a bare `HashSet::new()` (the let annotation supplies T) —
        // a `{ … }` block with no inserts would trip clippy's unused_mut.
        Expr::SetLit(elems) => {
            if elems.is_empty() {
                out.push_str("std::collections::HashSet::new()");
            } else {
                out.push_str("{ let mut __xset = std::collections::HashSet::new(); ");
                for e in elems {
                    out.push_str("__xset.insert(");
                    emit_expr(out, e, mode)?;
                    out.push_str("); ");
                }
                out.push_str("__xset }");
            }
        }
        // PMAT-500: Python `x in s` → `<set>.contains(&(<elem>))`.
        Expr::SetContains { set, elem } => {
            emit_expr(out, set, mode)?;
            out.push_str(".contains(&(");
            emit_expr(out, elem, mode)?;
            out.push_str("))");
        }
        // PMAT-502an: Python `x in xs` (list) → `(xs).contains(&(x))`.
        Expr::ListContains { list, elem } => {
            out.push('(');
            emit_expr(out, list, mode)?;
            out.push_str(").contains(&(");
            emit_expr(out, elem, mode)?;
            out.push_str("))");
        }
        // PMAT-502o: Python `sub in s` (str) → `(s).contains(&(sub)[..])`.
        Expr::StrContains { haystack, needle } => {
            out.push('(');
            emit_expr(out, haystack, mode)?;
            out.push_str(").contains(&(");
            emit_expr(out, needle, mode)?;
            out.push_str(")[..])");
        }
        // PMAT-502g: set algebra → `(lhs).<method>(&(rhs)).cloned().collect()`
        // into a fresh `HashSet`.
        Expr::SetOp { lhs, op, rhs } => {
            let method = match op {
                SetOp::Union => "union",
                SetOp::Intersection => "intersection",
                SetOp::Difference => "difference",
                SetOp::SymmetricDifference => "symmetric_difference",
            };
            out.push('(');
            emit_expr(out, lhs, mode)?;
            out.push_str(").");
            out.push_str(method);
            out.push_str("(&(");
            emit_expr(out, rhs, mode)?;
            out.push_str(")).cloned().collect::<std::collections::HashSet<_>>()");
        }
        // PMAT-502ep: set predicate → a parenthesized temp-bound block over
        // `HashSet::is_subset`/`is_superset`/`is_disjoint` (proper variants add
        // `&& __l != __r`). Temps avoid double-evaluating either operand.
        Expr::SetPred { lhs, op, rhs } => {
            // PMAT-652: bind the operands BY REFERENCE, not by value. Binding
            // `let __l = <set var>` moves the set, so `a <= b` (and the
            // self-comparison `a <= a`) failed with E0382 when the operand was
            // reused. `&(expr)` borrows an Ident operand and extends a temporary
            // operand (set-union etc.) via temporary-lifetime-extension.
            out.push_str("({ let __l = &(");
            emit_expr(out, lhs, mode)?;
            out.push_str("); let __r = &(");
            emit_expr(out, rhs, mode)?;
            out.push_str("); ");
            out.push_str(match op {
                SetPredOp::Subset => "__l.is_subset(__r)",
                SetPredOp::Superset => "__l.is_superset(__r)",
                SetPredOp::Disjoint => "__l.is_disjoint(__r)",
                SetPredOp::ProperSubset => "__l.is_subset(__r) && __l != __r",
                SetPredOp::ProperSuperset => "__l.is_superset(__r) && __l != __r",
            });
            out.push_str(" })");
        }
        // PMAT-502eq: `xs.copy()` / `d.copy()` / `s.copy()` → `(<inner>).clone()`.
        Expr::Clone(inner) => {
            out.push('(');
            emit_expr(out, inner, mode)?;
            out.push_str(").clone()");
        }
        // PMAT-502ew: `Option` value — `None` / `Some(<e>)`.
        Expr::OptionExpr(inner) => match inner {
            None => out.push_str("None"),
            Some(e) => {
                out.push_str("Some(");
                emit_expr(out, e, mode)?;
                out.push(')');
            }
        },
        // PMAT-721 (HUNT-V9 V9-18): Optional truthiness →
        // `(<value>)[.as_ref()].is_some_and(|__v| <body>)`. `as_ref()` is used for
        // a non-Copy inner so the value is borrowed, not consumed; `Expr::Len`
        // emits a uniform `.len()` that works on the `&__v` the closure receives.
        Expr::OptionTruthy {
            value,
            by_ref,
            body,
        } => {
            out.push('(');
            emit_expr(out, value, mode)?;
            out.push(')');
            if *by_ref {
                out.push_str(".as_ref()");
            }
            out.push_str(".is_some_and(|__v| ");
            emit_expr(out, body, mode)?;
            out.push(')');
        }
        // PMAT-724 (HUNT-V9 V9-19): `x or default` over Optional →
        // `(value).filter(|<param>| <body>).unwrap_or_else(|| <default>)`. `filter`
        // hands the predicate a `&T`; the param is `&__v` for a Copy inner (so the
        // value-form body sees `__v: T`) and `__v` for a non-Copy inner (the
        // `Len`/`&`-borrowing body). `unwrap_or_else` keeps Python's short-circuit.
        Expr::OptionOrDefault {
            value,
            by_ref,
            body,
            default,
        } => {
            out.push('(');
            emit_expr(out, value, mode)?;
            out.push(')');
            if *by_ref {
                out.push_str(".filter(|__v| ");
            } else {
                out.push_str(".filter(|&__v| ");
            }
            emit_expr(out, body, mode)?;
            out.push_str(").unwrap_or_else(|| ");
            emit_expr(out, default, mode)?;
            out.push(')');
        }
        // PMAT-502ex: `x is None` → `(x).is_none()`; `x is not None` →
        // `(x).is_some()`.
        Expr::IsNone { value, negated } => {
            out.push('(');
            emit_expr(out, value, mode)?;
            out.push_str(if *negated {
                ").is_some()"
            } else {
                ").is_none()"
            });
        }
        // PMAT-502ez: a flow-narrowed Optional read → `(<inner>).unwrap()`.
        Expr::OptionUnwrap(inner) => {
            out.push('(');
            emit_expr(out, inner, mode)?;
            out.push_str(").unwrap()");
        }
        // PMAT-506b: struct construction `Name { f0: v0, … }`.
        Expr::StructLit { name, fields } => {
            out.push_str(name);
            out.push_str(" { ");
            for (i, (field, value)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write!(out, "{field}: ")?;
                emit_expr(out, value, mode)?;
            }
            out.push_str(" }");
        }
        // PMAT-506b: struct field read `(obj).field`.
        Expr::FieldAccess { obj, field } => {
            out.push('(');
            emit_expr(out, obj, mode)?;
            write!(out, ").{field}")?;
        }
        // PMAT-506d: struct method call `(obj).method(args)`.
        Expr::MethodCall { obj, method, args } => {
            out.push('(');
            emit_expr(out, obj, mode)?;
            write!(out, ").{method}(")?;
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr(out, a, mode)?;
            }
            out.push(')');
        }
        // PMAT-513: an enum member access `C::NAME`.
        Expr::EnumVariant { enum_name, variant } => write!(out, "{enum_name}::{variant}")?,
        // PMAT-503b: `try: return <body> except: return <handler>` → catch the
        // panics xpile raises for Python exceptions via `catch_unwind`.
        Expr::TryCatch {
            body,
            handler,
            except_types,
            bound_name,
        } => {
            // PMAT-817 (HUNT-V20 EXC-4): `except E as e:` binds the exception
            // MESSAGE — the `<msg>` of the `xpile: <T>: <msg>` payload, with the
            // `xpile: <T>: ` prefix stripped — to a `String` local `e`.
            let bind = |out: &mut String, name: &str| {
                write!(
                    out,
                    "let {name} = __xpile_m.strip_prefix(\"xpile: \").and_then(|__s| __s.splitn(2, \": \").nth(1)).unwrap_or(__xpile_m).to_string(); "
                )
            };
            out.push_str("match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| ");
            emit_expr(out, body, mode)?;
            out.push_str(")) { Ok(__xpile_try) => __xpile_try, ");
            // PMAT-789 (HUNT-V18 EXC-001): a handler discriminating a NON-EMPTY set
            // of types catches a panic ONLY when its payload names one of THAT set
            // (`xpile: <T>: …` for some `T` in `except_types`) and RE-RAISES
            // (resume_unwind) anything else. This is an ALLOWLIST — the inversion of
            // the prior blocklist (PMAT-731/763), which re-raised only the OTHER
            // cataloged builtins and so CAUGHT any non-cataloged exception
            // (RuntimeError, a custom error) and any untagged panic — silently
            // swallowing what Python propagates (`except ValueError:` ate a
            // RuntimeError). Now an unmatched payload propagates, matching CPython.
            // A catch-all handler (`except_types` empty — a bare `except:` or a
            // base-class `except Exception:`) keeps `Err(_)` and catches everything.
            if except_types.is_empty() {
                if let Some(name) = bound_name {
                    // catch-all with `as e` — downcast the payload to a message.
                    out.push_str("Err(__xpile_e) => { let __xpile_m: &str = __xpile_e.downcast_ref::<String>().map(|__s| __s.as_str()).or_else(|| __xpile_e.downcast_ref::<&str>().copied()).unwrap_or(\"\"); ");
                    bind(out, name)?;
                    emit_expr(out, handler, mode)?;
                    out.push_str(" }");
                } else {
                    out.push_str("Err(_) => ");
                    emit_expr(out, handler, mode)?;
                }
            } else {
                out.push_str("Err(__xpile_e) => { let __xpile_m: &str = __xpile_e.downcast_ref::<String>().map(|__s| __s.as_str()).or_else(|| __xpile_e.downcast_ref::<&str>().copied()).unwrap_or(\"\"); if ");
                for (i, k) in except_types.iter().enumerate() {
                    if i > 0 {
                        out.push_str(" || ");
                    }
                    write!(out, "__xpile_m.starts_with(\"xpile: {k}: \")")?;
                }
                out.push_str(" { ");
                if let Some(name) = bound_name {
                    bind(out, name)?;
                }
                emit_expr(out, handler, mode)?;
                out.push_str(" } else { ::std::panic::resume_unwind(__xpile_e) } }");
            }
            out.push_str(" }");
        }
        // PMAT-459 (v0.2.0 Track 1.B): Python `len(x)` → Rust
        // `x.len() as i64`. Vec/String both expose `.len()` returning
        // `usize`; the `as i64` cast brings the result back into
        // Python's signed-int domain.
        Expr::Len(inner) => {
            // PMAT-761 (HUNT-V16 CFD-3): parenthesize the cast. A bare
            // `x.len() as i64` in a comparison — `if len(x) < N` — makes rustc
            // read `i64 <` as the start of generic arguments (a turbofish), a
            // hard parse error. Wrapping as `(x.len() as i64)` disambiguates it
            // in every position (the int()-cast arm already parenthesizes).
            out.push('(');
            emit_expr(out, inner, mode)?;
            out.push_str(".len() as i64)");
        }
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => emit_if_expr(out, cond, then_expr, else_expr, mode)?,
        Expr::Call { callee, args } => emit_call(out, callee, args, mode)?,
        Expr::UnOp { op, operand } => emit_unop(out, *op, operand, mode)?,
        // PMAT-449 (v0.2.0 Track 1.A): Python `str` literals lower
        // to owned `String::from("...")`. The character set is
        // escape-aware (`"` and `\` → `\"` / `\\`); v0.2.0 starts
        // with the minimal escape set, expanded in later sub-tracks.
        Expr::LitStr(s) => {
            write!(out, "String::from(\"{}\")", escape_rust_str(s))?;
        }
        // PMAT-042: `QuotedString` carries an explicit shell-domain
        // quoting strategy (bareword vs single-quote vs double-quote);
        // its semantics are bashrs-only. Rust backend refuses.
        Expr::QuotedString { .. } => {
            return Err(CodegenError::Unsupported(
                "Rust backend does not lower Expr::QuotedString — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs quoted shell strings; \
                 use `--target shell`"
                    .into(),
            ));
        }
        // PMAT-045: shell-variable references — same disposition.
        Expr::ShellVar(name) => {
            return Err(CodegenError::Unsupported(format!(
                "Rust backend does not lower Expr::ShellVar (${name}) — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell variable references; \
                 use `--target shell`"
            )));
        }
        // PMAT-047: command substitution — same disposition.
        Expr::CommandSubstitution(_) => {
            return Err(CodegenError::Unsupported(
                "Rust backend does not lower Expr::CommandSubstitution — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell substitution; \
                 use `--target shell`"
                    .into(),
            ));
        }
        // PMAT-055: shell special parameters — same disposition.
        Expr::ShellSpecial(name) => {
            return Err(CodegenError::Unsupported(format!(
                "Rust backend does not lower Expr::ShellSpecial (${name}) — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell special params; \
                 use `--target shell`"
            )));
        }
    }
    Ok(())
}

fn emit_unop(out: &mut String, op: UnOp, operand: &Expr, mode: bool) -> Result<(), CodegenError> {
    match op {
        UnOp::Neg => {
            if mode {
                // BigInt::neg returns BigInt without overflow risk.
                // PMAT-012 — slow-path side of C-PY-INT-ARITH.
                write!(out, "(-")?;
                emit_expr(out, operand, mode)?;
                write!(out, ")")?;
            } else {
                // Python: `-x` on int never overflows mathematically (int is unbounded).
                // Rust: `i64::MIN.checked_neg() == None`. Use checked_neg + panic that
                // points at the unimplemented bigint promotion slow path of
                // contract C-PY-INT-ARITH. See py-int-arith-v1.yaml.
                write!(out, "(")?;
                emit_expr(out, operand, mode)?;
                write!(
                    out,
                    ").checked_neg().expect(\"xpile: i64 negation overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")"
                )?;
            }
        }
        UnOp::Not => {
            write!(out, "(!")?;
            emit_expr(out, operand, mode)?;
            write!(out, ")")?;
        }
        // PMAT-502fb: Python `~x` == `-(x+1)` == Rust `!x` on a signed integer.
        UnOp::BitNot => {
            write!(out, "(!(")?;
            emit_expr(out, operand, mode)?;
            write!(out, "))")?;
        }
    }
    Ok(())
}

fn emit_call(
    out: &mut String,
    callee: &str,
    args: &[Expr],
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "{}(", callee)?;
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            write!(out, ", ")?;
        }
        emit_expr(out, a, mode)?;
    }
    write!(out, ")")?;
    Ok(())
}

/// Rust `if cond { then } else { else_ }` — usable as an expression.
/// When the `else_` is itself another `IfExpr`, emit a flat
/// `else if ...` form (no extra braces) for readability.
fn emit_if_expr(
    out: &mut String,
    cond: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "if ")?;
    emit_expr(out, cond, mode)?;
    write!(out, " {{ ")?;
    emit_expr(out, then_expr, mode)?;
    write!(out, " }} else ")?;
    match else_expr {
        Expr::IfExpr {
            cond: c2,
            then_expr: t2,
            else_expr: e2,
        } => {
            emit_if_expr(out, c2, t2, e2, mode)?;
            return Ok(());
        }
        _ => {
            write!(out, "{{ ")?;
            emit_expr(out, else_expr, mode)?;
            write!(out, " }}")?;
        }
    }
    Ok(())
}

/// Emit a binary op.
///
/// Arithmetic (`+`, `-`, `*`, `//`, `%`) uses `checked_*` variants with
/// `.expect("…")` rather than wrapping/truncating. Python `int` is
/// mathematically unbounded — silently wrapping at i64 would violate
/// the Layer-1 contract `C-PY-INT-ARITH`. Until the contract's bigint
/// slow path is implemented, overflow panics with a message pointing
/// at the contract.
///
/// FloorDiv / Mod additionally preserve Python-floor semantics via
/// `checked_div_euclid` / `checked_rem_euclid` (plain `/` and `%` in
/// Rust truncate toward zero, which diverges from Python on negative
/// operands).
///
/// Comparisons and logical ops never overflow, so they remain infix.
fn emit_binop(
    out: &mut String,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    match op {
        // Arithmetic: in BigInt mode all of these are plain infix
        // (BigInt overloads `+ - * <= ...` via num-bigint) — no
        // overflow risk, so no `.checked_*().expect(...)`. The C-PY-INT-ARITH
        // slow path is satisfied directly. PMAT-012.
        BinOp::Add if mode => emit_infix(out, lhs, " + ", rhs, mode),
        BinOp::Sub if mode => emit_infix(out, lhs, " - ", rhs, mode),
        BinOp::Mul if mode => emit_infix(out, lhs, " * ", rhs, mode),
        BinOp::FloorDiv if mode => emit_bigint_floor_call(out, "div_floor", lhs, rhs, mode),
        BinOp::Mod if mode => emit_bigint_floor_call(out, "mod_floor", lhs, rhs, mode),
        // PMAT-026 / PMAT-013-FOLLOWUP: bitwise + shift + power on
        // BigInt. num-bigint's `BitAnd / BitOr / BitXor` are direct
        // infix operators on `BigInt`; `<< >>` and `**` take rhs as
        // `usize` / `u32` (not BigInt), so we route through helpers
        // in `xpile_bigint::{shl, shr, pow}` that handle the
        // BigInt → primitive conversion (with a contract-named panic
        // on out-of-range exponents — same posture as the i64 fast
        // path's shift / pow handling).
        BinOp::BitAnd if mode => emit_infix(out, lhs, " & ", rhs, mode),
        BinOp::BitOr if mode => emit_infix(out, lhs, " | ", rhs, mode),
        BinOp::BitXor if mode => emit_infix(out, lhs, " ^ ", rhs, mode),
        BinOp::Shl if mode => emit_bigint_floor_call(out, "shl", lhs, rhs, mode),
        BinOp::Shr if mode => emit_bigint_floor_call(out, "shr", lhs, rhs, mode),
        BinOp::Pow if mode => emit_bigint_floor_call(out, "pow", lhs, rhs, mode),
        BinOp::Add => emit_checked(out, lhs, "checked_add", rhs, "addition", mode),
        BinOp::Sub => emit_checked(out, lhs, "checked_sub", rhs, "subtraction", mode),
        BinOp::Mul => emit_checked(out, lhs, "checked_mul", rhs, "multiplication", mode),
        // PMAT-538: `div_euclid`/`rem_euclid` only match Python `//`/`%` for a
        // POSITIVE divisor. Python `//` floors toward −∞ and `%` takes the sign
        // of the divisor; for a negative divisor the euclidean ops diverge
        // (e.g. `-7 // -2` is 3 in Python but `div_euclid` gives 4). Emit the
        // truncating quotient/remainder with a floor correction instead.
        BinOp::FloorDiv => emit_floor_div(out, lhs, rhs, mode),
        BinOp::Mod => emit_floor_mod(out, lhs, rhs, mode),
        // PMAT-618: `d.get(k) == v` / `!= v`. A no-default `d.get(k)` is
        // `Option<T>`, so comparing it to a bare value `v` is `Option<T> == T`
        // (E0308). Python's `d.get(k)` is `None` when absent (`None == v` is
        // False), which Rust models exactly as `Option<T> == Some(v)` — wrap the
        // NON-Option side in `Some(...)`. Only `==`/`!=` (a `<`/`>` on a
        // possibly-`None` is a Python `TypeError`); both-Option compares already
        // typecheck and fall through to the plain infix arm below.
        BinOp::Eq if is_dict_get_opt(lhs) ^ is_dict_get_opt(rhs) => {
            emit_opt_eq(out, lhs, " == ", rhs, mode)
        }
        BinOp::NotEq if is_dict_get_opt(lhs) ^ is_dict_get_opt(rhs) => {
            emit_opt_eq(out, lhs, " != ", rhs, mode)
        }
        BinOp::Eq => emit_infix(out, lhs, " == ", rhs, mode),
        BinOp::NotEq => emit_infix(out, lhs, " != ", rhs, mode),
        BinOp::Lt => emit_infix(out, lhs, " < ", rhs, mode),
        BinOp::LtEq => emit_infix(out, lhs, " <= ", rhs, mode),
        BinOp::Gt => emit_infix(out, lhs, " > ", rhs, mode),
        BinOp::GtEq => emit_infix(out, lhs, " >= ", rhs, mode),
        BinOp::And => emit_infix(out, lhs, " && ", rhs, mode),
        BinOp::Or => emit_infix(out, lhs, " || ", rhs, mode),
        BinOp::BitAnd => emit_infix(out, lhs, " & ", rhs, mode),
        BinOp::BitOr => emit_infix(out, lhs, " | ", rhs, mode),
        BinOp::BitXor => emit_infix(out, lhs, " ^ ", rhs, mode),
        BinOp::Shl => emit_checked_shift(out, lhs, "checked_shl", rhs, "left-shift", mode),
        BinOp::Shr => emit_checked_shift(out, lhs, "checked_shr", rhs, "right-shift", mode),
        BinOp::Pow => emit_checked_pow(out, lhs, rhs, mode),
    }
}

/// BigInt-mode floor-div / mod via the helpers exposed in xpile-bigint.
/// Takes references because num-bigint's `Integer::div_floor` consumes
/// `self`; the wrappers borrow. PMAT-012.
fn emit_bigint_floor_call(
    out: &mut String,
    method: &str,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "xpile_bigint::{method}(&")?;
    emit_expr(out, lhs, mode)?;
    write!(out, ", &")?;
    emit_expr(out, rhs, mode)?;
    write!(out, ")")?;
    Ok(())
}

/// Emit `(lhs).checked_pow(u32::try_from(rhs).expect(...)).expect(...)`.
/// Same panic-naming pattern as shifts: the inner expect fires on a
/// negative exponent (Python would return Float, which v0.1.0's type
/// system has no I64-compatible representation for); the outer expect
/// fires on i64 overflow.
fn emit_checked_pow(
    out: &mut String,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs, mode)?;
    write!(out, ").checked_pow(u32::try_from(")?;
    emit_expr(out, rhs, mode)?;
    write!(
        out,
        ").expect(\"xpile: exponent out of range for u32 — Python returns Float for negative exponents which v0.1.0 cannot represent (contract C-PY-INT-ARITH)\")).expect(\"xpile: i64 power overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")"
    )?;
    Ok(())
}

/// Emit a shift: `(lhs).checked_sh*(u32::try_from(rhs).expect(...)).expect(...)`.
/// Both panics name `C-PY-INT-ARITH` so the trail is still legible — the
/// inner one fires when Python's "shift by negative or huge" raises in
/// CPython; the outer one fires when the shift amount is >= 64 on i64.
fn emit_checked_shift(
    out: &mut String,
    lhs: &Expr,
    method: &str,
    rhs: &Expr,
    op_name: &str,
    mode: bool,
) -> Result<(), CodegenError> {
    // PMAT-575: `checked_shl` only validates the shift *amount* (`None` iff the
    // amount is >= 64); it does NOT detect VALUE overflow. So `1i64 << 63`
    // returns `Some(i64::MIN)` and the `.expect(... overflow ...)` never fires —
    // a silent wrap that falsifies C-PY-INT-ARITH's overflow guarantee (Python's
    // `<<` is exact / arbitrary-precision, so the contract promises a panic until
    // bigint promotion lands). Emit a reversibility check: a left shift loses no
    // significant bits iff `(v << n) >> n == v` (arithmetic shift-back, valid for
    // both signs). Right-shift never value-overflows, and bigint mode is
    // arbitrary-precision, so both keep the plain checked form.
    if method == "checked_shl" && !mode {
        write!(out, "{{ let __shl_v: i64 = ")?;
        emit_expr(out, lhs, mode)?;
        write!(out, "; let __shl_n: u32 = u32::try_from(")?;
        emit_expr(out, rhs, mode)?;
        write!(
            out,
            ").expect(\"xpile: shift amount out of range for u32 (contract C-PY-INT-ARITH)\"); let __shl_r = __shl_v.checked_shl(__shl_n).expect(\"xpile: i64 left-shift overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); if (__shl_r >> __shl_n) != __shl_v {{ panic!(\"xpile: i64 left-shift overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); }} __shl_r }}"
        )?;
        return Ok(());
    }
    // PMAT-577: Python defines `x >> n` for ANY non-negative `n` — once `n`
    // reaches the bit width the result saturates to the sign fill (`0` for
    // `x >= 0`, `-1` for `x < 0`, since `>>` is arithmetic on a signed int).
    // Rust's `checked_shr` returns `None` for `n >= 64`, so the `.expect`
    // panicked where Python returns a value. Clamp the amount to 63 (which
    // yields exactly that sign fill); a NEGATIVE amount still panics, matching
    // Python's `ValueError: negative shift count`.
    if method == "checked_shr" && !mode {
        write!(out, "{{ let __shr_v: i64 = ")?;
        emit_expr(out, lhs, mode)?;
        write!(out, "; let __shr_n: i64 = ")?;
        emit_expr(out, rhs, mode)?;
        write!(
            out,
            "; if __shr_n < 0 {{ panic!(\"xpile: negative shift amount (Python ValueError: negative shift count; contract C-PY-INT-ARITH)\"); }} let __shr_amt: u32 = if __shr_n >= 64 {{ 63 }} else {{ __shr_n as u32 }}; __shr_v >> __shr_amt }}"
        )?;
        return Ok(());
    }
    write!(out, "(")?;
    emit_expr(out, lhs, mode)?;
    write!(out, ").{method}(u32::try_from(")?;
    emit_expr(out, rhs, mode)?;
    write!(
        out,
        ").expect(\"xpile: shift amount out of range for u32 (contract C-PY-INT-ARITH)\")).expect(\"xpile: i64 {op_name} overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")"
    )?;
    Ok(())
}

/// Emit a checked binary op: `(<lhs>).<method>(<rhs>).expect("<msg> overflow ...")`.
/// Returns `i64`, identical to infix on the no-overflow fast path.
fn emit_checked(
    out: &mut String,
    lhs: &Expr,
    method: &str,
    rhs: &Expr,
    op_name: &str,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs, mode)?;
    write!(out, ").{method}(")?;
    emit_expr(out, rhs, mode)?;
    write!(
        out,
        ").expect(\"xpile: i64 {op_name} overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")"
    )?;
    Ok(())
}

/// PMAT-538: Python floor-division `a // b` for i64. Rust `/` truncates toward
/// zero and `div_euclid` keeps a non-negative remainder; neither matches
/// Python's floor (toward −∞) for a negative divisor. Emit the truncating
/// quotient plus a floor correction (subtract 1 when the remainder is non-zero
/// and its sign differs from the divisor's). `checked_div`/`checked_rem` keep
/// the `i64::MIN / -1` and divide-by-zero panics (same contract posture as the
/// former `checked_div_euclid`); the `__q - 1` correction is only reached when
/// the remainder is non-zero, where `__q` is never `i64::MIN`, so it cannot
/// overflow.
fn emit_floor_div(
    out: &mut String,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    let panic_msg = "xpile: i64 floor-div overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented";
    write!(out, "{{ let __fa = ")?;
    emit_expr(out, lhs, mode)?;
    write!(out, "; let __fb = ")?;
    emit_expr(out, rhs, mode)?;
    // PMAT-728 (HUNT-V10 typed-exceptions sub-slice): guard the zero divisor with
    // Python's ZeroDivisionError message BEFORE `checked_div` — `checked_div`
    // returns None for BOTH a zero divisor and the `i64::MIN / -1` overflow, so the
    // old single `.expect("...overflow...")` reported a misleading "overflow" for
    // the common `a // 0` (Python raises ZeroDivisionError). Mirrors the float path.
    write!(
        out,
        "; if __fb == 0 {{ panic!(\"xpile: ZeroDivisionError: integer division or modulo by zero\"); }} \
         let __q = __fa.checked_div(__fb).expect(\"{panic_msg}\"); \
         let __r = __fa.checked_rem(__fb).expect(\"{panic_msg}\"); \
         if __r != 0 && (__r < 0) != (__fb < 0) {{ __q - 1 }} else {{ __q }} }}"
    )?;
    Ok(())
}

/// PMAT-538: Python modulo `a % b` for i64. Python's result takes the sign of
/// the divisor; Rust `%` takes the sign of the dividend and `rem_euclid` is
/// always non-negative — both diverge for a negative divisor. Emit the
/// truncating remainder plus a floor correction (add the divisor when the
/// remainder is non-zero and its sign differs). The corrected value has
/// magnitude < |divisor|, so `__r + __fb` cannot overflow.
/// PMAT-740 (HUNT-V12 V12-24): emit an i64-typed expression cast up to `i128`,
/// recursing through a `*` tree so the *whole* product is computed in i128 (no
/// intermediate i64 `checked_mul`). A leaf is `(<expr> as i128)`; a `Mul`
/// becomes `(<a_i128> * <b_i128>)`. Used by `emit_floor_mod` to widen `(a*b) % m`.
fn emit_mul_tree_as_i128(out: &mut String, e: &Expr, mode: bool) -> Result<(), CodegenError> {
    if let Expr::BinOp {
        op: BinOp::Mul,
        lhs,
        rhs,
    } = e
    {
        out.push('(');
        emit_mul_tree_as_i128(out, lhs, mode)?;
        out.push_str(" * ");
        emit_mul_tree_as_i128(out, rhs, mode)?;
        out.push(')');
    } else {
        out.push('(');
        emit_expr(out, e, mode)?;
        out.push_str(" as i128)");
    }
    Ok(())
}

fn emit_floor_mod(
    out: &mut String,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    let panic_msg = "xpile: i64 modulo overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented";
    // PMAT-740 (HUNT-V12 V12-24): `(a * b) % m` — the product a*b commonly
    // overflows i64 even when the modular result fits (modular arithmetic,
    // rolling hashes, `(a*b) % MOD`). Widen the whole product AND the floor-mod
    // to i128 so the intermediate never overflows. `(a*b) mod m` lies in
    // (-|m|, |m|) ⊆ i64, so the final `as i64` is exact (mirrors the 3-arg pow
    // i128 path). Bigint mode already promotes, so only the non-bigint i64 path
    // needs this.
    if !mode && matches!(lhs, Expr::BinOp { op: BinOp::Mul, .. }) {
        write!(out, "{{ let __mm: i128 = ")?;
        emit_mul_tree_as_i128(out, lhs, mode)?;
        write!(out, "; let __md: i128 = (")?;
        emit_expr(out, rhs, mode)?;
        write!(
            out,
            ") as i128; if __md == 0 {{ panic!(\"xpile: ZeroDivisionError: integer modulo by zero\"); }} \
             let __r = __mm % __md; \
             (if __r != 0 && (__r < 0) != (__md < 0) {{ __r + __md }} else {{ __r }}) as i64 }}"
        )?;
        return Ok(());
    }
    write!(out, "{{ let __fa = ")?;
    emit_expr(out, lhs, mode)?;
    write!(out, "; let __fb = ")?;
    emit_expr(out, rhs, mode)?;
    // PMAT-728: guard the zero divisor with Python's ZeroDivisionError message
    // before `checked_rem` (which also returns None on `i64::MIN % -1` overflow).
    write!(
        out,
        "; if __fb == 0 {{ panic!(\"xpile: ZeroDivisionError: integer modulo by zero\"); }} \
         let __r = __fa.checked_rem(__fb).expect(\"{panic_msg}\"); \
         if __r != 0 && (__r < 0) != (__fb < 0) {{ __r + __fb }} else {{ __r }} }}"
    )?;
    Ok(())
}

/// PMAT-745 (HUNT-V13 intfloat-cmp-precision): emit an EXACT `int OP float`
/// comparison. Python never rounds the int operand to `f64`, so a magnitude
/// above 2^53 (where consecutive integers are no longer distinct in `f64`) must
/// not be conflated with its rounded image. The block binds the int (`__cn`)
/// and float (`__cf`) once, then compares `__cn as f64` (`__cnf`) against
/// `__cf` for STRICT ordering — reliable because a rounded integer `t` lands
/// within half a ULP of `__cnf`, so `__cnf < __cf` (as floats) implies `t <
/// __cf` too (and symmetrically for `>`) — and breaks the `__cnf == __cf` tie
/// in `i128`, which exactly holds every integral `f64` an `i64` cast can reach
/// (up to 2^63), so the boundary cases (`i64::MAX` vs `2^63`, `2**53 + 1` vs
/// `2^53`) resolve correctly. NaN falls through every arm (Python: `n != nan`
/// is `True`, the rest `False`). The `op` is pre-normalised by the frontend to
/// the int-on-left form, so only the six comparison operators appear here.
fn emit_mixed_int_float_cmp(
    out: &mut String,
    int: &Expr,
    float: &Expr,
    op: BinOp,
    mode: bool,
) -> Result<(), CodegenError> {
    let body = match op {
        BinOp::Eq => "__cnf == __cf && (__cn as i128) == (__cf as i128)",
        BinOp::NotEq => "__cnf != __cf || (__cn as i128) != (__cf as i128)",
        BinOp::Lt => "__cnf < __cf || (__cnf == __cf && (__cn as i128) < (__cf as i128))",
        BinOp::LtEq => "__cnf < __cf || (__cnf == __cf && (__cn as i128) <= (__cf as i128))",
        BinOp::Gt => "__cnf > __cf || (__cnf == __cf && (__cn as i128) > (__cf as i128))",
        BinOp::GtEq => "__cnf > __cf || (__cnf == __cf && (__cn as i128) >= (__cf as i128))",
        other => {
            return Err(CodegenError::Unsupported(format!(
                "MixedIntFloatCmp with non-comparison operator {other:?} (frontend bug)"
            )))
        }
    };
    // A `let` RHS is a full expression up to `;`, so no operand parens are
    // needed (and they would warn `unused_parens` for a bare ident operand).
    out.push_str("{ let __cn = ");
    emit_expr(out, int, mode)?;
    out.push_str("; let __cf = ");
    emit_expr(out, float, mode)?;
    out.push_str("; let __cnf = __cn as f64; ");
    out.push_str(body);
    out.push_str(" }");
    Ok(())
}

fn emit_infix(
    out: &mut String,
    lhs: &Expr,
    op: &str,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs, mode)?;
    out.push_str(op);
    emit_expr(out, rhs, mode)?;
    write!(out, ")")?;
    Ok(())
}

/// PMAT-618: is this expression a no-default `d.get(k)` (an `Option<T>`)?
fn is_dict_get_opt(e: &Expr) -> bool {
    matches!(e, Expr::DictGetOpt { .. })
}

/// PMAT-618: emit an `==`/`!=` where exactly one operand is a no-default
/// `d.get(k)` (`Option<T>`). The Option side is emitted as-is; the bare-value
/// side is wrapped in `Some(...)` so the comparison is `Option<T> == Some(v)`,
/// matching Python (`None == v` is `False`).
fn emit_opt_eq(
    out: &mut String,
    lhs: &Expr,
    op: &str,
    rhs: &Expr,
    mode: bool,
) -> Result<(), CodegenError> {
    write!(out, "(")?;
    emit_opt_eq_operand(out, lhs, mode)?;
    out.push_str(op);
    emit_opt_eq_operand(out, rhs, mode)?;
    write!(out, ")")?;
    Ok(())
}

fn emit_opt_eq_operand(out: &mut String, e: &Expr, mode: bool) -> Result<(), CodegenError> {
    if is_dict_get_opt(e) {
        emit_expr(out, e, mode)
    } else {
        out.push_str("Some(");
        emit_expr(out, e, mode)?;
        out.push(')');
        Ok(())
    }
}

pub struct RustBackend;

impl Backend for RustBackend {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Rust]
    }

    fn lower(&self, module: &Module, _config: &BackendConfig) -> Result<Artifact, BackendError> {
        let primary = emit_module(module).map_err(|e| BackendError::Lower(e.to_string()))?;
        Ok(Artifact {
            primary,
            sidecars: Vec::new(),
            citations: Vec::new(),
            quorum_status: QuorumStatus::Single {
                emitter: "xpile-rust-codegen".to_string(),
            },
        })
    }
}

// ── C emit path (PMAT-467, v0.2.0 Track 2.A) ────────────────────────
//
// Isolated from the Python/Ruchy emit above so C's semantics can't
// regress it. C `int` is fixed-width `i32`; signed overflow is UB, for
// which `wrapping_*` is the sound conservative discharge (it produces a
// deterministic two's-complement result rather than invoking Rust UB).
// This mirrors the standalone-decy → C-C-INT-ARITH plan in
// `sub/v0.2.0-decy-merger.md`; the contract substrate is queued.

/// PMAT-909/910: the Rust scalar width a C function is emitted at. C `int`
/// (`Type::I64`) → `i32` (fixed-width, wrapping); C `long`/`int64_t`
/// (`Type::CLong`) → `i64`; C `double` (`Type::F64`) → `f64` (IEEE
/// arithmetic, no wrapping). A single function is emitted at ONE width —
/// "widest wins" with precedence `f64 > i64 > i32`: if its return, any
/// param, or any local is `F64`, the whole function rides `f64`; else if
/// any is `CLong`, it rides `i64`; else `i32`. For the integer widths this
/// is value-preserving (int fits in i64, only the wrap width changes). The
/// float case targets the uniformly-`double` C functions decy currently
/// produces; a mixed int/double function (C usual-arithmetic promotion) is
/// a deferred edge — decy has no fixture for one yet.
#[derive(Clone, Copy)]
struct CWidth {
    rust_ty: &'static str,
    lit_suffix: &'static str,
    /// `true` for the `f64` width: arithmetic is plain infix IEEE
    /// (`+ - * / %`), NOT the integer `wrapping_*` methods, and unary
    /// minus is `-(x)` not `wrapping_neg`.
    is_float: bool,
}

const C_WIDTH_I32: CWidth = CWidth {
    rust_ty: "i32",
    lit_suffix: "i32",
    is_float: false,
};
const C_WIDTH_I64: CWidth = CWidth {
    rust_ty: "i64",
    lit_suffix: "i64",
    is_float: false,
};
const C_WIDTH_F64: CWidth = CWidth {
    rust_ty: "f64",
    lit_suffix: "f64",
    is_float: true,
};

fn c_function_width(f: &Function) -> CWidth {
    let any_f64 = matches!(f.return_type, Type::F64)
        || f.params.iter().any(|p| matches!(p.ty, Type::F64))
        || c_stmts_have_ty(&f.body.stmts, &Type::F64);
    if any_f64 {
        return C_WIDTH_F64;
    }
    let any_clong = matches!(f.return_type, Type::CLong)
        || f.params.iter().any(|p| matches!(p.ty, Type::CLong))
        || c_stmts_have_ty(&f.body.stmts, &Type::CLong);
    if any_clong {
        C_WIDTH_I64
    } else {
        C_WIDTH_I32
    }
}

/// Does any `let` in `stmts` (recursing into `while`/`if` bodies) declare a
/// local of type `want`? Drives the PMAT-909/910 "widest wins" width pick.
fn c_stmts_have_ty(stmts: &[Stmt], want: &Type) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Let { ty, .. } => ty == want,
        Stmt::While { body, .. } => c_stmts_have_ty(body, want),
        Stmt::If {
            then_body,
            else_body,
            ..
        } => c_stmts_have_ty(then_body, want) || c_stmts_have_ty(else_body, want),
        _ => false,
    })
}

fn emit_c_function(out: &mut String, f: &Function) -> Result<(), CodegenError> {
    let w = c_function_width(f);
    if w.is_float {
        // PMAT-910: a C `double` function uses IEEE f64 arithmetic, NOT the
        // two's-complement wrapping `C-C-INT-ARITH` models. Its governing
        // contract (C-C-FLOAT-ARITH) is a queued R6 head — deliberately NOT
        // emitted as a `// xpile-contract:` line so the citation-integrity
        // gate (PMAT-475) never sees a phantom id. Plain comment only.
        writeln!(
            out,
            "// xpile-arith: C double -> IEEE f64 (C-C-FLOAT-ARITH queued, uncited)"
        )?;
    } else {
        // C int/long arithmetic is governed by the on-disk C-C-INT-ARITH.
        writeln!(out, "// xpile-contract: C-C-INT-ARITH")?;
    }
    write!(out, "pub fn {}(", f.name)?;
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            write!(out, ", ")?;
        }
        write!(out, "{}: {}", p.name, w.rust_ty)?;
    }
    writeln!(out, ") -> {} {{", w.rust_ty)?;
    for stmt in &f.body.stmts {
        emit_c_stmt(out, stmt, "    ", w)?;
    }
    write!(out, "    ")?;
    emit_c_expr(out, &f.body.trailing_return, w)?;
    writeln!(out)?;
    writeln!(out, "}}")?;
    Ok(())
}

fn emit_c_stmt(out: &mut String, stmt: &Stmt, indent: &str, w: CWidth) -> Result<(), CodegenError> {
    match stmt {
        Stmt::Let {
            name,
            value,
            mutable,
            ..
        } => {
            let kw = if *mutable { "let mut" } else { "let" };
            write!(out, "{indent}{kw} {name}: {} = ", w.rust_ty)?;
            emit_c_expr(out, value, w)?;
            writeln!(out, ";")?;
            Ok(())
        }
        Stmt::Assign { name, value } => {
            write!(out, "{indent}{name} = ")?;
            emit_c_expr(out, value, w)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-479 (R10): C early `return <expr>;` (guard clause).
        Stmt::Return(e) => {
            write!(out, "{indent}return ")?;
            emit_c_expr(out, e, w)?;
            writeln!(out, ";")?;
            Ok(())
        }
        Stmt::While { cond, body } => {
            write!(out, "{indent}while ")?;
            emit_c_expr(out, cond, w)?;
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in body {
                emit_c_stmt(out, s, &inner, w)?;
            }
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
        // PMAT-478 (R9): C `if (c) { … } else { … }` → Rust if/else
        // statement (the `else` block omitted when empty).
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            write!(out, "{indent}if ")?;
            emit_c_expr(out, cond, w)?;
            writeln!(out, " {{")?;
            let inner = format!("{indent}    ");
            for s in then_body {
                emit_c_stmt(out, s, &inner, w)?;
            }
            if else_body.is_empty() {
                writeln!(out, "{indent}}}")?;
            } else {
                writeln!(out, "{indent}}} else {{")?;
                for s in else_body {
                    emit_c_stmt(out, s, &inner, w)?;
                }
                writeln!(out, "{indent}}}")?;
            }
            Ok(())
        }
        other => Err(CodegenError::Unsupported(format!(
            "C backend supports `int x = e;`, `x = e;`, `if (c) {{ … }} else {{ … }}`, and `while (c) {{ … }}`, got {other:?}"
        ))),
    }
}

fn emit_c_expr(out: &mut String, e: &Expr, w: CWidth) -> Result<(), CodegenError> {
    match e {
        // PMAT-909/910: the literal suffix tracks the function width (`i32`
        // for C `int`, `i64` for `long`/`int64_t`, `f64` for `double`) so
        // the body is internally type-consistent. An int literal in a
        // float-width function emits as `<v>f64` (valid Rust, e.g. `2f64`).
        Expr::LitInt(v) => write!(out, "{v}{}", w.lit_suffix)?,
        // PMAT-910: a C float literal renders as a Rust f64 literal. `{}`
        // of a whole-valued f64 (`2.0`) prints `2`, so suffix with `f64`.
        Expr::LitFloat(v) => write!(out, "{v}f64")?,
        Expr::Ident(name) => write!(out, "{name}")?,
        Expr::BinOp { op, lhs, rhs } => emit_c_binop(out, *op, lhs, rhs, w)?,
        Expr::UnOp { op, operand } => match op {
            // C unary minus on `int` is wrapping (INT_MIN negation is UB
            // in C; `wrapping_neg` is the sound deterministic discharge).
            // PMAT-910: on `double` it is plain IEEE negation `-(x)`.
            UnOp::Neg if w.is_float => {
                write!(out, "-(")?;
                emit_c_expr(out, operand, w)?;
                write!(out, ")")?;
            }
            UnOp::Neg => {
                write!(out, "(")?;
                emit_c_expr(out, operand, w)?;
                write!(out, ").wrapping_neg()")?;
            }
            UnOp::Not => {
                write!(out, "!(")?;
                emit_c_expr(out, operand, w)?;
                write!(out, ")")?;
            }
            // PMAT-502fb: bitwise invert — Rust `!` on a signed integer.
            UnOp::BitNot => {
                write!(out, "!(")?;
                emit_c_expr(out, operand, w)?;
                write!(out, ")")?;
            }
        },
        Expr::IfExpr {
            cond,
            then_expr,
            else_expr,
        } => {
            write!(out, "if ")?;
            emit_c_expr(out, cond, w)?;
            write!(out, " {{ ")?;
            emit_c_expr(out, then_expr, w)?;
            write!(out, " }} else {{ ")?;
            emit_c_expr(out, else_expr, w)?;
            write!(out, " }}")?;
        }
        Expr::Call { callee, args } => {
            write!(out, "{callee}(")?;
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    write!(out, ", ")?;
                }
                emit_c_expr(out, a, w)?;
            }
            write!(out, ")")?;
        }
        other => {
            return Err(CodegenError::Unsupported(format!(
                "C backend slice 1 does not lower {other:?} — supported: int literals, \
                 identifiers, calls, + - *, comparisons, && ||, unary - !, and the ternary"
            )));
        }
    }
    Ok(())
}

fn emit_c_binop(
    out: &mut String,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    w: CWidth,
) -> Result<(), CodegenError> {
    // Arithmetic: wrapping (C signed overflow is UB → deterministic
    // two's-complement). Comparisons / logicals: plain infix, producing
    // a Rust `bool` (correct for `if`/`&&`/`||` operand positions, which
    // is where the C frontend places them). The `wrapping_*` methods are
    // width-agnostic in syntax — they wrap at the operand's width (i32 or
    // the PMAT-909 i64) without a suffix change.
    let wrapping = |out: &mut String, method: &str| -> Result<(), CodegenError> {
        write!(out, "(")?;
        emit_c_expr(out, lhs, w)?;
        write!(out, ").{method}(")?;
        emit_c_expr(out, rhs, w)?;
        write!(out, ")")?;
        Ok(())
    };
    let infix = |out: &mut String, sym: &str| -> Result<(), CodegenError> {
        emit_c_expr(out, lhs, w)?;
        write!(out, " {sym} ")?;
        emit_c_expr(out, rhs, w)?;
        Ok(())
    };
    match op {
        // PMAT-910: on the `double` width, C arithmetic is plain IEEE
        // infix (`+ - * /`) — f64 has no `wrapping_*` and never wraps. C
        // double `/` is true division (Rust f64 `/` matches); `%` on
        // doubles is not valid C, but if it appears f64 `%` is fmod-like.
        BinOp::Add if w.is_float => infix(out, "+")?,
        BinOp::Sub if w.is_float => infix(out, "-")?,
        BinOp::Mul if w.is_float => infix(out, "*")?,
        BinOp::FloorDiv if w.is_float => infix(out, "/")?,
        BinOp::Mod if w.is_float => infix(out, "%")?,
        BinOp::Add => wrapping(out, "wrapping_add")?,
        BinOp::Sub => wrapping(out, "wrapping_sub")?,
        BinOp::Mul => wrapping(out, "wrapping_mul")?,
        // C `/` truncates toward zero (Rust integer `/` does too);
        // `wrapping_div`/`wrapping_rem` add the INT_MIN/-1 UB guard.
        // The frontend carries these as FloorDiv/Mod (shared IR
        // variants); here they mean C truncating div/rem, not Python
        // floor.
        BinOp::FloorDiv => wrapping(out, "wrapping_div")?,
        BinOp::Mod => wrapping(out, "wrapping_rem")?,
        BinOp::Eq => infix(out, "==")?,
        BinOp::NotEq => infix(out, "!=")?,
        BinOp::Lt => infix(out, "<")?,
        BinOp::LtEq => infix(out, "<=")?,
        BinOp::Gt => infix(out, ">")?,
        BinOp::GtEq => infix(out, ">=")?,
        BinOp::And => infix(out, "&&")?,
        BinOp::Or => infix(out, "||")?,
        other => {
            return Err(CodegenError::Unsupported(format!(
                "C backend slice 1 does not lower BinOp::{other:?} — `/`, `%`, bitwise, \
                 shift, and power are deferred to a later decy slice"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use xpile_meta_hir::{Module, SourceLang};

    fn module_with(name: &str, items: Vec<Item>) -> Module {
        Module {
            name: name.into(),
            source_lang: SourceLang::Python,
            items,
            ffi_boundaries: Vec::new(),
        }
    }

    fn add_fn() -> Function {
        Function {
            name: "add".into(),
            params: vec![
                Param {
                    name: "a".into(),
                    ty: Type::I64,
                    mutable: false,
                },
                Param {
                    name: "b".into(),
                    ty: Type::I64,
                    mutable: false,
                },
            ],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                },
            },
        }
    }

    #[test]
    fn emits_add_function() {
        let m = module_with("fixture", vec![Item::Function(add_fn())]);
        let rust = emit_module(&m).expect("emit ok");
        assert!(rust.contains("pub fn add(a: i64, b: i64) -> i64"));
        // After contract C-PY-INT-ARITH was wired in (PMAT-002),
        // addition emits `(a).checked_add(b).expect(...)`, not plain
        // `(a + b)`. Assert on the load-bearing invariants rather
        // than the exact shape.
        assert!(rust.contains("checked_add"), "expected checked_add: {rust}");
        assert!(
            rust.contains("C-PY-INT-ARITH"),
            "expected contract reference in panic msg: {rust}"
        );
    }

    #[test]
    fn emits_floordiv_with_floor_correction() {
        // PMAT-538: Python `a // b` floors toward −∞ (so the result diverges
        // from `div_euclid` for a negative divisor). The emit must use the
        // truncating quotient (`checked_div`) plus a floor correction, NOT
        // `div_euclid` and NOT a bare Rust `/`.
        let f = Function {
            name: "fdiv".into(),
            params: vec![
                Param {
                    name: "a".into(),
                    ty: Type::I64,
                    mutable: false,
                },
                Param {
                    name: "b".into(),
                    ty: Type::I64,
                    mutable: false,
                },
            ],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::BinOp {
                    op: BinOp::FloorDiv,
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                },
            },
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let rust = emit_module(&m).expect("emit ok");
        assert!(
            rust.contains("checked_div") && rust.contains("__q - 1"),
            "Python floor-div must lower to checked_div + floor correction (got: {rust})"
        );
        assert!(
            !rust.contains("div_euclid"),
            "must not use div_euclid (wrong for a negative divisor): {rust}"
        );
    }

    #[test]
    fn emits_comparison_returning_bool() {
        let f = Function {
            name: "le".into(),
            params: vec![
                Param {
                    name: "a".into(),
                    ty: Type::I64,
                    mutable: false,
                },
                Param {
                    name: "b".into(),
                    ty: Type::I64,
                    mutable: false,
                },
            ],
            return_type: Type::Bool,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::BinOp {
                    op: BinOp::LtEq,
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                },
            },
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let rust = emit_module(&m).expect("emit ok");
        assert!(rust.contains("-> bool"));
        assert!(rust.contains("(a <= b)"));
    }

    #[test]
    fn emits_if_expression_for_ternary() {
        let f = Function {
            name: "pick".into(),
            params: vec![
                Param {
                    name: "a".into(),
                    ty: Type::I64,
                    mutable: false,
                },
                Param {
                    name: "b".into(),
                    ty: Type::I64,
                    mutable: false,
                },
            ],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::IfExpr {
                    cond: Box::new(Expr::BinOp {
                        op: BinOp::LtEq,
                        lhs: Box::new(Expr::Ident("a".into())),
                        rhs: Box::new(Expr::Ident("b".into())),
                    }),
                    then_expr: Box::new(Expr::Ident("a".into())),
                    else_expr: Box::new(Expr::Ident("b".into())),
                },
            },
        };
        let m = module_with("fixture", vec![Item::Function(f)]);
        let rust = emit_module(&m).expect("emit ok");
        assert!(rust.contains("if (a <= b) { a } else { b }"));
        assert!(rust.contains("pub fn pick(a: i64, b: i64) -> i64"));
    }

    #[test]
    fn emit_module_produces_rustc_parseable_output() {
        // Run the emitted source through `syn::parse_file` to ensure
        // syntactic well-formedness without spawning rustc. Doesn't
        // type-check (that's the workspace-test job).
        // (syn isn't a dep here; instead check basic shape.)
        let m = module_with("fixture", vec![Item::Function(add_fn())]);
        let rust = emit_module(&m).expect("emit ok");
        // sanity: balanced braces and trailing newline
        assert_eq!(rust.matches('{').count(), rust.matches('}').count());
        assert!(rust.ends_with('\n'));
    }

    fn c_module(name: &str, f: Function) -> Module {
        Module {
            name: name.into(),
            source_lang: SourceLang::C,
            items: vec![Item::Function(f)],
            ffi_boundaries: Vec::new(),
        }
    }

    #[test]
    fn c_emit_long_function_rides_i64_width() {
        // PMAT-909: a `long`-typed C function emits at i64 width — i64
        // signature, i64-suffixed literals, i64 wrapping. A pure-`int`
        // function stays at i32.
        let widen = Function {
            name: "widen".into(),
            params: vec![Param {
                name: "x".into(),
                ty: Type::CLong,
                mutable: false,
            }],
            return_type: Type::CLong,
            body: Block {
                stmts: vec![Stmt::Let {
                    name: "acc".into(),
                    ty: Type::CLong,
                    value: Expr::BinOp {
                        op: BinOp::Add,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::LitInt(1)),
                    },
                    mutable: false,
                }],
                trailing_return: Expr::Ident("acc".into()),
            },
        };
        let rust = emit_module(&c_module("w", widen)).expect("emit ok");
        assert!(
            rust.contains("pub fn widen(x: i64) -> i64"),
            "long C fn must ride i64 width: {rust}"
        );
        assert!(rust.contains("let acc: i64 ="), "long local is i64: {rust}");
        assert!(rust.contains("1i64"), "literal suffix tracks width: {rust}");
        assert!(rust.contains("wrapping_add"), "C arithmetic stays wrapping");
        assert!(!rust.contains("i32"), "no i32 leaks into a long fn: {rust}");
    }

    #[test]
    fn c_emit_int_function_stays_i32() {
        // Regression guard: PMAT-909 must NOT widen a pure-`int` C function.
        let rust = emit_module(&c_module("a", add_fn())).expect("emit ok");
        assert!(
            rust.contains("pub fn add(a: i32, b: i32) -> i32"),
            "int C fn stays i32: {rust}"
        );
        assert!(
            !rust.contains("i64"),
            "no i64 widening for a pure-int fn: {rust}"
        );
    }

    #[test]
    fn c_emit_double_function_rides_f64() {
        // PMAT-910: a `double`-typed C function emits at f64 width — f64
        // signature, IEEE infix arithmetic (NOT wrapping_*), f64 literals.
        let scale = Function {
            name: "scale".into(),
            params: vec![Param {
                name: "x".into(),
                ty: Type::F64,
                mutable: false,
            }],
            return_type: Type::F64,
            body: Block {
                stmts: vec![Stmt::Let {
                    name: "y".into(),
                    ty: Type::F64,
                    value: Expr::BinOp {
                        op: BinOp::Mul,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::LitFloat(2.0)),
                    },
                    mutable: false,
                }],
                trailing_return: Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Ident("y".into())),
                    rhs: Box::new(Expr::LitFloat(0.5)),
                },
            },
        };
        let rust = emit_module(&c_module("s", scale)).expect("emit ok");
        assert!(
            rust.contains("pub fn scale(x: f64) -> f64"),
            "double C fn must ride f64 width: {rust}"
        );
        assert!(rust.contains("let y: f64 ="), "double local is f64: {rust}");
        assert!(rust.contains("2f64"), "f64 literal suffix: {rust}");
        assert!(rust.contains("0.5f64"), "fractional f64 literal: {rust}");
        // IEEE infix, NOT integer wrapping.
        assert!(
            rust.contains("x * 2f64") && rust.contains("y + 0.5f64"),
            "float arithmetic is plain infix: {rust}"
        );
        assert!(
            !rust.contains("wrapping_"),
            "no integer wrapping in an f64 fn: {rust}"
        );
        assert!(
            !rust.contains("i32") && !rust.contains("i64"),
            "no int width leak: {rust}"
        );
        // PMAT-910 honesty: a double fn obeys IEEE semantics, not the
        // int-wrapping C-C-INT-ARITH — so it must cite NO contract (and
        // never emit a phantom `// xpile-contract:` id for the gate).
        assert!(
            !rust.contains("// xpile-contract:"),
            "double fn must emit no contract citation (C-C-FLOAT-ARITH queued): {rust}"
        );
    }
}
