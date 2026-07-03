//! Ruchy backend.
//!
//! Lowers meta-HIR to Ruchy source. v0.1.0 emits the same arithmetic
//! subset as `xpile-rust-codegen` — the surface difference is
//! `fun ... -> T { ... }` instead of Rust's `pub fn ... -> T { ... }`,
//! and floor-div / modulo still go through Euclidean semantics
//! (`div_euclid` / `rem_euclid`).
//!
//! Future scope (tracked by `Profile::RuchyOut`): reconstruct the
//! pipeline operator `|>` and DataFrame-flavored sugar from meta-HIR
//! patterns. See `docs/specifications/sub/bidirectional-ruchy.md`.

use std::fmt::Write;
use xpile_backend::{Artifact, Backend, BackendConfig, BackendError, QuorumStatus, Target};
use xpile_meta_hir::{
    BinOp, Block, DictViewKind, Expr, FloatOp, Function, Item, ListMutateOp, ListQueryOp, Module,
    NumBuiltinOp, Param, Radix, SetOp, SetPredOp, SourceLang, Stmt, StrMethodOp, Type, UnOp,
};

// PMAT-789 (HUNT-V18 EXC-001): the typed-`except` discriminator is now an
// allowlist (matches the handler's own listed types, re-raises everything else),
// so the `KNOWN_EXC` blocklist roster (PMAT-731) is no longer needed (mirror of
// the Rust backend).

/// PMAT-477 (R8): Ruchy → Rust infix symbol for a float arithmetic op.
/// PMAT-502by: escape a string for embedding inside a `println!`/`print!`
/// format-string literal (see the Rust backend's twin). Used by `print`'s
/// `sep=`/`end=` kwargs.
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
            // PMAT-748 (HUNT-V14 #3): other C0/DEL control chars → `\u{..}`.
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}

fn float_op_sym(op: FloatOp) -> &'static str {
    match op {
        FloatOp::Add => "+",
        FloatOp::Sub => "-",
        FloatOp::Mul => "*",
        FloatOp::Div => "/",
        // FloorDiv/Mod/Pow + math method-ops use dedicated formulas — keep the
        // match exhaustive.
        FloatOp::FloorDiv => "//",
        FloatOp::Mod => "%",
        FloatOp::Pow => "**",
        FloatOp::Hypot => "hypot",
        FloatOp::Atan2 => "atan2",
        FloatOp::Log => "log",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuchyCodegenError {
    #[error("unsupported item: {0}")]
    Unsupported(String),
    #[error("formatting error: {0}")]
    Format(#[from] std::fmt::Error),
}

pub fn emit_module(module: &Module) -> Result<String, RuchyCodegenError> {
    // PMAT-573: escape Rust-keyword identifiers on a cloned IR before
    // emission (Ruchy shares Rust's keyword set + raw-identifier `r#`
    // syntax). See the Rust backend's twin and `escape_rust_reserved_idents`.
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
    // PMAT-967: C sources lower with C arithmetic semantics (fixed-width
    // `i32`/`i64`/`u32`/`u64`, two's-complement wrapping overflow; IEEE `f64`/
    // `f32` on the float widths) via an isolated emit path — the exact twin of
    // the Rust backend's `is_c` branch, surface-shifted to Ruchy's `fun ... ->`
    // header. WITHOUT this, a `SourceLang::C` module routed through the
    // Python/Ruchy `emit_function` and silently emitted checked-`i64`/BigInt
    // arithmetic for a `int add(int, int)` — a mis-emit (wrong wrap width AND
    // panic-on-overflow instead of C wraparound). Governed by `C-C-INT-ARITH`
    // (integer widths) / `C-C-FLOAT-ARITH` (float widths), same as Rust.
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
            // PMAT-505a (classes epic, first cut): dataclass → derived struct
            // (Ruchy compiles to Rust — same shape).
            Item::Struct {
                name,
                fields,
                methods,
                frozen,
                order,
            } => {
                // PMAT-592/648: frozen dataclass → derive Eq, Hash (all fields
                // Eq+Hash-capable); order=True → derive PartialOrd. Matches the
                // Rust backend.
                let all_ord_fields = fields
                    .iter()
                    .all(|(_, ty)| matches!(ty, Type::I64 | Type::Bool | Type::Str));
                let derive_eq_hash = *frozen && all_ord_fields;
                // PMAT-750 (HUNT-V14 #6): order=True over all-Ord-able fields also
                // derives Ord (+ Eq) so instances can be .sort()ed (Vec::sort needs
                // Ord; PartialOrd alone is E0277). A float field can't derive Ord.
                let derive_ord = *order && all_ord_fields;
                // PMAT-762 (HUNT-V16 DD-01): a custom `__eq__` overrides `==` via
                // an `impl PartialEq` (below); suppress the structural derives
                // (mirror of the Rust backend).
                let has_custom_eq = methods.iter().any(|m| m.name == "__eq__");
                // PMAT-777 (HUNT-V17 #3): a custom __ne__ also needs a hand impl
                // PartialEq (to set `fn ne`); suppress the derive when either present.
                let has_custom_ne = methods.iter().any(|m| m.name == "__ne__");
                let custom_eq_impl = has_custom_eq || has_custom_ne;
                // PMAT-769 (HUNT-V16 DD-07): a custom __lt__ → generated impl
                // PartialOrd (below); suppress the order=True structural derive.
                // PMAT-791 (HUNT-V18 #11): a custom order dunder (lt/gt/ge/le)
                // suppresses the structural order derive and gets a hand
                // `impl PartialOrd` (mirror of the Rust backend).
                let order_dunder = ["__lt__", "__gt__", "__ge__", "__le__"]
                    .into_iter()
                    .find(|d| methods.iter().any(|m| m.name == *d));
                let has_order_dunder = order_dunder.is_some();
                let mut derives = vec!["Clone", "Debug"];
                if !custom_eq_impl {
                    derives.push("PartialEq");
                    if derive_eq_hash || (derive_ord && !has_order_dunder) {
                        derives.push("Eq");
                    }
                    if derive_eq_hash {
                        derives.push("Hash");
                    }
                }
                if *order && !has_order_dunder {
                    derives.push("PartialOrd");
                }
                if derive_ord && !has_order_dunder {
                    derives.push("Ord");
                }
                // PMAT-958 (Pillar-A definition-level citation closure): cite the
                // class→struct contract on the struct DEFINITION itself (mirror of
                // the Rust backend), so a method-less `@dataclass` no longer ships
                // `pub struct {..}` uncited. Derived from `Item::applicable_contracts`.
                emit_item_contract_citations(&mut out, item)?;
                writeln!(out, "#[derive({})]", derives.join(", "))?;
                writeln!(out, "pub struct {name} {{")?;
                for (field, ty) in fields {
                    write!(out, "    pub {field}: ")?;
                    emit_type(&mut out, ty)?;
                    out.push_str(",\n");
                }
                out.push_str("}\n");
                // PMAT-760 (HUNT-V15 #6): generate a Python-repr `Display` for an
                // all-int/bool dataclass (mirror of the Rust backend) so an
                // instance renders in an f-string / str() / print() instead of
                // E0277 (struct derives only Debug).
                // PMAT-776 (HUNT-V17 #2): a custom `__str__` becomes the Display
                // (delegating), taking precedence over the field-repr (mirror of
                // the Rust backend).
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
                // PMAT-840 (HUNT-V26 #9): a float field also formats the same in the
                // dataclass repr as str(float) — generate Display for an
                // int/bool/FLOAT dataclass (a str field is still deferred). Mirrors
                // the Rust backend.
                // PMAT-841 (HUNT-V26 #9): a str field also renders (quoted) in the
                // dataclass repr; mirrors the Rust backend.
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
                        // PMAT-810: the repr LABEL shows the Python field name (strip
                        // the keyword-field `r#`); the `self.{field}` access keeps it.
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
                // PMAT-506d: instance methods → an `impl` block (Ruchy → Rust).
                if !methods.is_empty() {
                    writeln!(out, "impl {name} {{")?;
                    for m in methods {
                        emit_function(&mut out, m)?;
                    }
                    out.push_str("}\n");
                }
                // PMAT-762 (HUNT-V16 DD-01): delegate `==` to a custom `__eq__`
                // (mirror of the Rust backend).
                if custom_eq_impl {
                    writeln!(out, "impl PartialEq for {name} {{")?;
                    writeln!(out, "    fn eq(&self, __other: &Self) -> bool {{")?;
                    if has_custom_eq {
                        writeln!(out, "        self.__eq__(__other.clone())")?;
                    } else if fields.is_empty() {
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
                    out.push_str("    }\n");
                    if has_custom_ne {
                        writeln!(out, "    fn ne(&self, __other: &Self) -> bool {{")?;
                        writeln!(out, "        self.__ne__(__other.clone())")?;
                        out.push_str("    }\n");
                    }
                    out.push_str("}\n");
                }
                // PMAT-769/791 (HUNT-V16 DD-07 / HUNT-V18 #11): delegate ordering to
                // a custom order dunder (lt/gt/ge/le) via a generated impl
                // PartialOrd (mirror of the Rust backend).
                if let Some(d) = order_dunder {
                    let body = match d {
                        "__lt__" => "if self.__lt__(__other.clone()) { Some(std::cmp::Ordering::Less) } else if __other.__lt__(self.clone()) { Some(std::cmp::Ordering::Greater) } else { Some(std::cmp::Ordering::Equal) }",
                        "__gt__" => "if self.__gt__(__other.clone()) { Some(std::cmp::Ordering::Greater) } else if __other.__gt__(self.clone()) { Some(std::cmp::Ordering::Less) } else { Some(std::cmp::Ordering::Equal) }",
                        "__ge__" => "if self.__ge__(__other.clone()) { if __other.__ge__(self.clone()) { Some(std::cmp::Ordering::Equal) } else { Some(std::cmp::Ordering::Greater) } } else { Some(std::cmp::Ordering::Less) }",
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
            }
            // PMAT-513: a Python `Enum` class → a Rust enum (Ruchy → Rust).
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

fn emit_function(out: &mut String, f: &Function) -> Result<(), RuchyCodegenError> {
    emit_contract_citations(out, f)?;
    // Ruchy: `fun name(params) -> ret { body }`. No `pub`.
    write!(out, "fun {}(", f.name)?;
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

/// PMAT-012-FOLLOWUP / PMAT-025: a function is in BigInt mode if any
/// param is BigInt, the return type is BigInt, OR any pre-bound Let
/// is BigInt. In BigInt mode, the Ruchy backend emits the same shape
/// as the Rust backend (since Ruchy compiles to Rust):
/// `xpile_bigint::BigInt::from(<n>i64)` literals + plain infix
/// arithmetic + `.clone()` on Ident references (BigInt isn't `Copy`).
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
            // PMAT-479 (R10): early return introduces no BigInt binding.
            // PMAT-494b: tuple unpacking introduces no BigInt binding.
            // PMAT-503a: a raise introduces no BigInt binding.
            Stmt::Assign { .. }
            | Stmt::Assert { .. }
            | Stmt::Return(_)
            | Stmt::LetTuple { .. }
            // PMAT-1016A: a side-effect call introduces no BigInt binding.
            | Stmt::SideEffectCall { .. }
            | Stmt::ClosureLet { .. }
            // PMAT-736: a named inner fn is never BigInt-typed at v0.2.0.
            | Stmt::NestedFn { .. }
            | Stmt::Continue
            | Stmt::Break
            // PMAT-502bw: print() introduces no binding.
            | Stmt::Print { .. }
            | Stmt::Raise { .. } => false,
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
            // PMAT-1058: statement-form try/except — a bigint op in either arm.
            Stmt::TryCatch { body, handler, .. } => {
                body.iter().any(stmt_has_bigint) || handler.iter().any(stmt_has_bigint)
            }
            // PMAT-460: list.append() — same disposition. PMAT-502ap/aq/ar:
            // in-place list mutators / extend / insert likewise carry no binding.
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
            // PMAT-1037: field subscript store carries no Type::Let.
            Stmt::FieldIndexAssign { .. } => false,
            // PMAT-533: subscript-receiver append carries no Type::Let.
            Stmt::IndexAppend { .. } => false,
            // PMAT-727: setdefault-append carries no Type::Let.
            Stmt::DictSetdefaultAppend { .. } => false,
            // PMAT-466: dict keyed assignment same disposition.
            Stmt::DictSet { .. } => false,
            // PMAT-506c: field assignment introduces no binding.
            Stmt::FieldAssign { .. } => false,
            // PMAT-502at: del coll[key] introduces no binding.
            Stmt::DelItem { .. } => false,
            // PMAT-039: see rust-codegen's twin arm — shell commands
            // carry no BigInt operands.
            Stmt::Cmd { .. } => false,
            Stmt::FileWrite { .. } => false,
            // PMAT-041: see rust-codegen's twin arm.
            Stmt::Pipeline { .. } => false,
            // PMAT-048: see rust-codegen's twin arm.
            Stmt::ShellLoop { .. } => false,
            // PMAT-051: see rust-codegen's twin arm.
            Stmt::ShellAssign { .. } => false,
        }
    }
    f.body.stmts.iter().any(stmt_has_bigint)
}

/// PMAT-840 (HUNT-V26 #9): CPython-faithful f64 `repr`/`str` as a Rust block over
/// `accessor` — mirrors the Rust backend's `py_float_repr_block` (Ruchy compiles
/// to Rust). Raw-string template with `__ACC__` substituted; used by the dataclass
/// `Display` generator for a float field.
fn py_float_repr_block(accessor: &str) -> String {
    let tmpl = r##"{ let __sf = __ACC__; if __sf.is_nan() { String::from("nan") } else if __sf.is_infinite() { String::from(if __sf < 0.0 { "-inf" } else { "inf" }) } else { let __se = format!("{:e}", __sf); let __ep = __se.find('e').unwrap(); let __ex: i32 = __se[__ep + 1..].parse().unwrap(); if __ex < -4 || __ex >= 16 { format!("{}e{}{:02}", &__se[..__ep], if __ex < 0 { "-" } else { "+" }, __ex.abs()) } else if __sf.fract() == 0.0 { format!("{}.0", __sf) } else { format!("{}", __sf) } } }"##;
    tmpl.replace("__ACC__", accessor)
}

/// PMAT-841 (HUNT-V26 #9): CPython-faithful `repr(str)` as a Rust block over
/// `accessor` — mirrors the Rust backend's `py_str_repr_block` (Ruchy compiles to
/// Rust). Used by the dataclass `Display` generator for a str field.
/// PMAT-1091: same extended non-printable escape predicate as the Rust twin
/// (Cc + Zl/Zp/Zs + Cf incl. astral + Co + noncharacters, CPython-width
/// `\x`/`\u`/`\U` forms); general unassigned Cn stays the documented gap.
fn py_str_repr_block(accessor: &str) -> String {
    let tmpl = r##"{ let __rs = &(__ACC__); let __q = if __rs.contains('\'') && !__rs.contains('"') { '"' } else { '\'' }; let mut __ro = String::new(); __ro.push(__q); for __rc in __rs.chars() { match __rc { '\\' => { __ro.push('\\'); __ro.push('\\'); } '\n' => { __ro.push('\\'); __ro.push('n'); } '\r' => { __ro.push('\\'); __ro.push('r'); } '\t' => { __ro.push('\\'); __ro.push('t'); } __ec if __ec == __q => { __ro.push('\\'); __ro.push(__ec); } __ec if { let __u = __ec as u32; __u < 0x20 || (0x7f..=0xa0).contains(&__u) || __u == 0xad || (0x600..=0x605).contains(&__u) || __u == 0x61c || __u == 0x6dd || __u == 0x70f || (0x890..=0x891).contains(&__u) || __u == 0x8e2 || __u == 0x1680 || __u == 0x180e || (0x2000..=0x200f).contains(&__u) || (0x2028..=0x202f).contains(&__u) || (0x205f..=0x206f).contains(&__u) || __u == 0x3000 || (0xe000..=0xf8ff).contains(&__u) || (0xfdd0..=0xfdef).contains(&__u) || __u == 0xfeff || (0xfff9..=0xfffb).contains(&__u) || __u == 0x110bd || __u == 0x110cd || (0x13430..=0x1343f).contains(&__u) || (0x1bca0..=0x1bca3).contains(&__u) || (0x1d173..=0x1d17a).contains(&__u) || __u == 0xe0001 || (0xe0020..=0xe007f).contains(&__u) || __u >= 0xf0000 || (__u & 0xfffe) == 0xfffe } => { let __u = __ec as u32; if __u < 0x100 { __ro.push_str(&format!("\\x{:02x}", __u)); } else if __u < 0x10000 { __ro.push_str(&format!("\\u{:04x}", __u)); } else { __ro.push_str(&format!("\\U{:08x}", __u)); } } __ec => __ro.push(__ec) } } __ro.push(__q); __ro }"##;
    tmpl.replace("__ACC__", accessor)
}

/// PMAT-1089: the CPython-shaped `KeyError` panic over a pre-bound key
/// reference `__k` — mirrors the Rust backend's `key_error_panic` (Ruchy
/// compiles to Rust): `str(KeyError(k))` is `repr(k)`, so a string key carries
/// the quote-switched repr and a bool key Python's `True`/`False` casing.
/// PMAT-1091: composite (tuple) keys repr recursively via the same
/// autoref-specialization block as the Rust twin — block-local
/// `__XpileKeyRepr` over i64/bool/String/&str + 1/2/3-tuples, `Debug`
/// fallback for anything else (see the Rust backend's doc for the mechanism).
fn key_error_panic() -> String {
    let tmpl = r##"panic!("xpile: KeyError: {}", { #[allow(dead_code)] trait __XpileKeyRepr { fn __xkr(&self) -> String; } impl __XpileKeyRepr for i64 { fn __xkr(&self) -> String { format!("{}", self) } } impl __XpileKeyRepr for bool { fn __xkr(&self) -> String { String::from(if *self { "True" } else { "False" }) } } impl __XpileKeyRepr for String { fn __xkr(&self) -> String { __STRREPR__ } } impl __XpileKeyRepr for &str { fn __xkr(&self) -> String { __STRREPR__ } } impl<__A: __XpileKeyRepr> __XpileKeyRepr for (__A,) { fn __xkr(&self) -> String { format!("({},)", self.0.__xkr()) } } impl<__A: __XpileKeyRepr, __B: __XpileKeyRepr> __XpileKeyRepr for (__A, __B) { fn __xkr(&self) -> String { format!("({}, {})", self.0.__xkr(), self.1.__xkr()) } } impl<__A: __XpileKeyRepr, __B: __XpileKeyRepr, __C: __XpileKeyRepr> __XpileKeyRepr for (__A, __B, __C) { fn __xkr(&self) -> String { format!("({}, {}, {})", self.0.__xkr(), self.1.__xkr(), self.2.__xkr()) } } #[allow(dead_code)] struct __Xw<'__x, __T>(&'__x __T); #[allow(dead_code)] trait __XkrVal { fn __xkrv(&self) -> String; } impl<'__x, __T: __XpileKeyRepr> __XkrVal for __Xw<'__x, __T> { fn __xkrv(&self) -> String { self.0.__xkr() } } #[allow(dead_code)] trait __XkrDbg { fn __xkrv(&self) -> String; } impl<'__x, __T: ::std::fmt::Debug> __XkrDbg for &__Xw<'__x, __T> { fn __xkrv(&self) -> String { format!("{:?}", self.0) } } (&__Xw(__k)).__xkrv() })"##;
    tmpl.replace("__STRREPR__", &py_str_repr_block("self"))
}

/// PMAT-011: same `// xpile-contract: <ID>` form as the Rust backend.
/// Ruchy compiles to Rust, so it shares the comment-citation convention.
fn emit_contract_citations(out: &mut String, f: &Function) -> Result<(), RuchyCodegenError> {
    for id in f.applicable_contracts() {
        writeln!(out, "// xpile-contract: {id}")?;
    }
    Ok(())
}

/// PMAT-958: definition-level analog of [`emit_contract_citations`] — emits
/// the `// xpile-contract: <ID>` line(s) governing an `Item` *definition*
/// (struct/const/enum). Derived from [`Item::applicable_contracts`], the
/// same source the citation-integrity gate reads.
fn emit_item_contract_citations(out: &mut String, item: &Item) -> Result<(), RuchyCodegenError> {
    for id in item.applicable_contracts() {
        writeln!(out, "// xpile-contract: {id}")?;
    }
    Ok(())
}

fn emit_block(out: &mut String, block: &Block, mode: bool) -> Result<(), RuchyCodegenError> {
    for stmt in &block.stmts {
        emit_stmt(out, stmt, mode)?;
    }
    write!(out, "    ")?;
    emit_expr(out, &block.trailing_return, mode)?;
    writeln!(out)?;
    Ok(())
}

/// PMAT-1080: does a loop body REASSIGN `name` (so the `for` binding needs
/// `mut`)? `for x in xs: x = x.strip()` → `for mut x`. Recurses if/while/nested
/// loops but stops at a nested loop that SHADOWS `name` (rebinds it — refused
/// at the frontend for depyler input since PMAT-1085, kept defensively).
/// Precise (an over-broad `mut` would trip clippy `unused_mut`).
/// PMAT-1085 (skeptic findings b/c): also recurses try/except/finally blocks
/// (a reassign buried in a `try:` was rustc E0594) and pair-loops (an outer
/// var reassigned inside `for k, v in …` was E0384). An `as e` handler binding
/// shadows `name` within that handler, like a nested loop binding does.
fn foreach_var_reassigned(body: &[Stmt], name: &str) -> bool {
    body.iter().any(|s| match s {
        Stmt::Assign { name: n, .. } => n == name,
        Stmt::If {
            then_body,
            else_body,
            ..
        } => foreach_var_reassigned(then_body, name) || foreach_var_reassigned(else_body, name),
        Stmt::While { body, .. } => foreach_var_reassigned(body, name),
        Stmt::ForEach { var, body, .. } => var != name && foreach_var_reassigned(body, name),
        Stmt::ForEachPair {
            first,
            second,
            body,
            ..
        } => first != name && second != name && foreach_var_reassigned(body, name),
        Stmt::ForEachZip3 {
            first,
            second,
            third,
            body,
            ..
        } => first != name && second != name && third != name && foreach_var_reassigned(body, name),
        Stmt::TryCatch {
            body,
            handler,
            bound_name,
            extra_handlers,
            finally,
            ..
        } => {
            foreach_var_reassigned(body, name)
                || (bound_name.as_deref() != Some(name) && foreach_var_reassigned(handler, name))
                || extra_handlers.iter().any(|h| {
                    h.bound_name.as_deref() != Some(name) && foreach_var_reassigned(&h.body, name)
                })
                || foreach_var_reassigned(finally, name)
        }
        _ => false,
    })
}

/// PMAT-1105 (c): the emitted catch-all gate — TRUE iff the downcast payload
/// `__xpile_m` is a PYTHON exception (`xpile: <Class>: …` with an
/// identifier-shaped class token). xpile's capability/honesty refusals panic
/// with FREE-TEXT payloads that fail this parse, so every handler — including
/// a bare `except:` — re-raises them: a loud refusal stays loud inside
/// try/except. Mirrors the Rust backend (Ruchy compiles to Rust).
const IS_PY_EXC_PRED: &str = "__xpile_m.strip_prefix(\"xpile: \").and_then(|__s| __s.split_once(\": \")).map_or(false, |(__c, _)| !__c.is_empty() && __c.chars().all(|__ch| __ch.is_ascii_alphanumeric() || __ch == '_'))";

/// PMAT-1105 (b): the BaseException-only exclusions for the sentinel
/// `Exception` tag — SystemExit / KeyboardInterrupt / GeneratorExit derive
/// BaseException, not Exception, so `except Exception:` must let them
/// propagate. Mirrors the Rust backend.
const NOT_BASE_ONLY_PRED: &str = "!__xpile_m.starts_with(\"xpile: SystemExit: \") && !__xpile_m.starts_with(\"xpile: KeyboardInterrupt: \") && !__xpile_m.starts_with(\"xpile: GeneratorExit: \")";

/// PMAT-1105: write the dispatch predicate for one `except` tag. Leaf tags
/// match their `xpile: <T>: ` prefix; the frontend's sentinel tag `Exception`
/// matches any Python-exception payload except the BaseException-only three.
fn write_exc_tag_pred(out: &mut String, tag: &str) -> std::fmt::Result {
    use std::fmt::Write as _;
    if tag == "Exception" {
        write!(out, "{IS_PY_EXC_PRED} && {NOT_BASE_ONLY_PRED}")
    } else {
        write!(out, "__xpile_m.starts_with(\"xpile: {tag}: \")")
    }
}

fn emit_stmt(out: &mut String, stmt: &Stmt, mode: bool) -> Result<(), RuchyCodegenError> {
    emit_stmt_indented(out, stmt, "    ", mode)
}

fn emit_stmt_indented(
    out: &mut String,
    stmt: &Stmt,
    indent: &str,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    match stmt {
        Stmt::Let {
            name,
            ty,
            value,
            mutable,
        } => {
            // PMAT-598: suppress the element-type annotation on a mutable empty
            // `set()` so rustc infers it from the later `.insert(...)` (matches
            // the Rust backend).
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
        // PMAT-494b: tuple unpacking → `let (a, b, ...) = <value>;`.
        Stmt::LetTuple {
            names,
            mutable,
            value,
        } => {
            // PMAT-547: mark each unpacked name `mut` per its `mutable` flag.
            // PMAT-662: never prefix the `_` wildcard with `mut` (see rust backend).
            // PMAT-1010: mask all-but-last duplicate targets to `_` (`a, a = 1, 2`
            // is Python last-wins; a twice-bound pattern name is E0416 — see rust
            // backend).
            let pat = names
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    if n != "_" && names[i + 1..].contains(n) {
                        "_".to_string()
                    } else if n != "_" && mutable.get(i).copied().unwrap_or(false) {
                        format!("mut {n}")
                    } else {
                        n.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            // PMAT-711: single-element unpack needs the trailing comma (mirror rust).
            let trailing = if names.len() == 1 { "," } else { "" };
            write!(out, "{indent}let ({pat}{trailing}) = ")?;
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-479 (R10): early `return <expr>;` (guard clause).
        Stmt::Return(e) => {
            write!(out, "{indent}return ")?;
            emit_expr(out, e, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-1016A: a statement-position side-effect call (mutating
        // user-class method / void fn) — emit `<call>;` (mirror rust).
        Stmt::SideEffectCall { call } => {
            write!(out, "{indent}")?;
            emit_expr(out, call, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-502bk: loop-control statements, matching the Rust backend.
        Stmt::Continue => {
            writeln!(out, "{indent}continue;")?;
            Ok(())
        }
        Stmt::Break => {
            writeln!(out, "{indent}break;")?;
            Ok(())
        }
        // PMAT-502bw/by: `print(a, b, …, sep=…, end=…)` (see the Rust
        // backend for the join/end logic).
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
        // PMAT-478 (R9): if/else statement → `if c { … } else { … }`.
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
        Stmt::Assign { name, value } => {
            write!(out, "{indent}{name} = ")?;
            emit_expr(out, value, mode)?;
            writeln!(out, ";")?;
            Ok(())
        }
        // PMAT-504: closure binding (0+ params), matching the Rust backend.
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
        // PMAT-736: a named inner fn item — `fn <name>(<params>) -> R { <body> }`,
        // matching the Rust backend. A real `fn` (not a closure) so a self-call
        // recurses by name. Always i64-mode (mode=false), independent of the
        // enclosing fn's bigint mode.
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
            let inner = format!("{indent}    ");
            for st in &body.stmts {
                emit_stmt_indented(out, st, &inner, false)?;
            }
            out.push_str(&inner);
            emit_expr(out, &body.trailing_return, false)?;
            writeln!(out)?;
            writeln!(out, "{indent}}}")?;
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
        // PMAT-458 (v0.2.0 Track 1.B): Ruchy → Rust → for-each with
        // .iter().cloned() for owned-value bindings.
        Stmt::ForEach {
            var,
            iter,
            body,
            over_keys,
            dict_guard,
            elem_ty: _,
            mutate_elems,
        } => {
            // PMAT-1080: `for x in xs: x = …` rebinds the loop var → `for mut x`.
            let vmut = if foreach_var_reassigned(body, var) {
                "mut "
            } else {
                ""
            };
            // PMAT-816 (HUNT-V21 #3/4/8): in-place element mutation → bind `var`
            // by `&mut` via `iter_mut()` so the mutation reaches the original
            // collection (mirrors the Rust backend).
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
            // PMAT-472 (R3): dict iterates keys via `.keys().cloned()`.
            let method = if *over_keys { "keys" } else { "iter" };
            // PMAT-743 (HUNT-V12 V12-8): dict size-change guard (mirrors Rust) —
            // a mutated dict whose keys were materialized panics like Python's
            // RuntimeError if its length changes mid-iteration; a value-update
            // (size-stable) is silent.
            if let Some(g) = dict_guard {
                writeln!(out, "{indent}{{ let __dg_n0 = {g}.len();")?;
                write!(out, "{indent}for {vmut}{var} in ")?;
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
            write!(out, "{indent}for {vmut}{var} in ")?;
            emit_expr(out, iter, mode)?;
            writeln!(out, ".{method}().cloned() {{")?;
            let inner = format!("{indent}    ");
            for s in body {
                emit_stmt_indented(out, s, &inner, mode)?;
            }
            writeln!(out, "{indent}}}")?;
            Ok(())
        }
        // PMAT-495: paired for-loop (enumerate / zip), Ruchy → Rust.
        Stmt::ForEachPair {
            first,
            second,
            iter,
            kind,
            body,
        } => {
            // PMAT-1085 (c): reassigned pair-loop tuple bindings need `mut`
            // (mirrors the Rust backend; was E0384).
            let m1 = if foreach_var_reassigned(body, first) {
                "mut "
            } else {
                ""
            };
            let m2 = if foreach_var_reassigned(body, second) {
                "mut "
            } else {
                ""
            };
            write!(out, "{indent}for ({m1}{first}, {m2}{second}) in ")?;
            emit_expr(out, iter, mode)?;
            match kind {
                xpile_meta_hir::PairIterKind::Enumerate { start } => {
                    // PMAT-502ca / PMAT-595: `enumerate(xs, start)` offsets the
                    // index; the offset add honors C-PY-INT-ARITH.
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
        // PMAT-562: three-way `zip` → nested `.zip()` chain + `((a, b), c)`.
        Stmt::ForEachZip3 {
            first,
            second,
            third,
            iter1,
            iter2,
            iter3,
            body,
        } => {
            // PMAT-1085 (c): reassigned zip3 tuple bindings need `mut` too.
            let m1 = if foreach_var_reassigned(body, first) {
                "mut "
            } else {
                ""
            };
            let m2 = if foreach_var_reassigned(body, second) {
                "mut "
            } else {
                ""
            };
            let m3 = if foreach_var_reassigned(body, third) {
                "mut "
            } else {
                ""
            };
            write!(
                out,
                "{indent}for (({m1}{first}, {m2}{second}), {m3}{third}) in "
            )?;
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
        // PMAT-460 (v0.2.0 Track 1.B): Ruchy → Rust → `.push(...)`.
        Stmt::ListAppend { list_name, elem } => {
            write!(out, "{indent}{list_name}.push(")?;
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-500b: Ruchy → Rust `s.insert(x);`.
        Stmt::SetAdd { set_name, elem } => {
            write!(out, "{indent}{set_name}.insert(")?;
            emit_expr(out, elem, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-502av: `s.remove(x)` / `s.discard(x)`, matching the Rust backend.
        Stmt::SetRemove {
            set_name,
            elem,
            error_if_absent,
        } => {
            if *error_if_absent {
                // PMAT-1089: CPython-shaped payload — `str(KeyError(x))` is
                // `repr(x)`, not a fixed "x not in set" text.
                write!(out, "{indent}{{ let __k = &(")?;
                emit_expr(out, elem, mode)?;
                writeln!(
                    out,
                    "); if !{set_name}.remove(__k) {{ {} }} }}",
                    key_error_panic()
                )?;
            } else {
                write!(out, "{indent}{set_name}.remove(&(")?;
                emit_expr(out, elem, mode)?;
                writeln!(out, "));")?;
            }
            Ok(())
        }
        // PMAT-502ap: in-place list mutators, matching the Rust backend.
        Stmt::ListMutate {
            list_name,
            op,
            of_float,
        } => {
            match op {
                // PMAT-616: NaN-safe float sort (Python doesn't raise on NaN).
                ListMutateOp::Sort if *of_float => writeln!(
                    out,
                    "{indent}{list_name}.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));"
                )?,
                ListMutateOp::Sort => writeln!(out, "{indent}{list_name}.sort();")?,
                // PMAT-555: descending in-place sort (`sort(reverse=True)`).
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
        // PMAT-502aq: `xs.extend(ys)`, matching the Rust backend.
        Stmt::ListExtend { list_name, other } => {
            write!(out, "{indent}{list_name}.extend((")?;
            emit_expr(out, other, mode)?;
            writeln!(out, ").iter().cloned());")?;
            Ok(())
        }
        // PMAT-502bb: `d.update(other)`, matching the Rust backend.
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
        // CPython `list.insert` semantics, matching the Rust backend.
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
        // PMAT-502eg: `xs.remove(x)` → position-find + remove, matching the
        // Rust backend (panics ≈ Python `ValueError` when absent).
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
        // PMAT-461 (v0.2.0 Track 1.B): Ruchy → Rust →
        // `xs[i as usize] = v;`, matching the Rust backend.
        Stmt::IndexAssign {
            list_name,
            indices,
            value,
        } => {
            // PMAT-640/641: any runtime index (not a non-negative literal), at
            // any nesting level, wraps like Python — mirrors the Rust backend.
            // Each level's index is staged into a temp first (using the
            // progressively-indexed collection's own `len`), which also ends the
            // collection's immutable borrow before the `index_mut` assign
            // (subsumes the old `needs_temps` self-referential path).
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
                    // PMAT-863 (HUNT-V30 #3): bounds-check the WRITE path (mirror
                    // the Rust backend) — out-of-range subscript-assign otherwise
                    // silently wrote a wrong slot. Python IndexError.
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
        // progressive `&mut` navigation then leaf assign (mirrors the Rust backend).
        Stmt::NestedSubscriptAssign { base, steps, value } => {
            emit_subscript_write_through(out, indent, base, steps, value, mode)
        }
        // PMAT-1037: subscript store through a struct field — same progressive
        // `&mut` walk with base `<obj>.<field>` (mirrors the Rust backend).
        Stmt::FieldIndexAssign {
            obj,
            field,
            steps,
            value,
        } => {
            emit_subscript_write_through(out, indent, &format!("{obj}.{field}"), steps, value, mode)
        }
        // PMAT-466 (v0.2.0 Track 1.C): Ruchy → Rust
        // `{ let __v = v; d.insert(k.clone(), __v); }`, matching the Rust
        // backend — value bound to a temp before insert, and the key
        // cloned so a non-Copy str key survives a later read (see the
        // Rust twin arm for the full move-then-borrow rationale).
        Stmt::DictSet {
            dict_name,
            key,
            value,
        } => {
            write!(out, "{indent}{{ let __xpile_dict_val = ")?;
            emit_expr(out, value, mode)?;
            // PMAT-852 (HUNT-V28 #4): parenthesize the key before `.clone()` so a
            // bare-cast key (`len(w)` → `… as i64`) doesn't become `… as
            // i64.clone()` (mirror the Rust backend).
            write!(out, "; {dict_name}.insert((")?;
            emit_expr(out, key, mode)?;
            writeln!(out, ").clone(), __xpile_dict_val); }}")?;
            Ok(())
        }
        // PMAT-533: append on a subscript receiver (mirrors the Rust twin).
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
        // `d.entry(k).or_insert_with(|| default).push(elem);` (mirrors rust backend).
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
        // PMAT-502at: `del coll[key]`, matching the Rust backend.
        Stmt::DelItem { name, key, is_dict } => {
            if *is_dict {
                // PMAT-709: `del d[k]` raises KeyError on an absent key (mirror rust).
                // PMAT-1089: CPython-shaped payload — `str(KeyError(k))` is `repr(k)`.
                write!(out, "{indent}{{ let __k = &(")?;
                emit_expr(out, key, mode)?;
                writeln!(
                    out,
                    "); if {name}.shift_remove(__k).is_none() {{ {} }} }}",
                    key_error_panic()
                )?;
            } else if expr_mentions_ident(key, name) {
                // PMAT-570: `del xs[-k]` index references `xs` — bind before remove.
                write!(out, "{indent}{{ let __di = (")?;
                emit_expr(out, key, mode)?;
                writeln!(out, ") as usize; {name}.remove(__di); }}")?;
            } else {
                // PMAT-712: normalize a runtime-negative index (mirror rust).
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
            // PMAT-788 (HUNT-V17 #4): emit a tagged `xpile: AssertionError:`
            // panic (mirror of the Rust backend) so a typed except discriminates it.
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
        // PMAT-503a: `raise Exc("msg")` → `panic!("{}", <message>);` (Ruchy
        // compiles to Rust and inherits the diverging-panic disposition).
        Stmt::Raise { message } => {
            write!(out, "{indent}panic!(\"{{}}\", ")?;
            emit_expr(out, message, mode)?;
            writeln!(out, ");")?;
            Ok(())
        }
        // PMAT-1058: statement-form try/except — mirrors the rust-codegen emit
        // (ruchy transpiles to Rust). Statement-block arms, `Ok(_) => {}`, the
        // PMAT-789 allowlist re-raise + PMAT-817 `as e` binding.
        Stmt::TryCatch {
            body,
            handler,
            except_types,
            bound_name,
            extra_handlers,
            finally,
            finally_only,
        } => {
            // PMAT-1073: `try: B finally: F` with NO except — no handler
            // dispatch; run body, finally, then propagate.
            if *finally_only {
                write!(
                    out,
                    "{indent}{{ let __tc_outer = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {{ "
                )?;
                for st in body {
                    emit_stmt_indented(out, st, "", mode)?;
                }
                out.push_str(" })); ");
                for st in finally {
                    emit_stmt_indented(out, st, "", mode)?;
                }
                out.push_str(
                    "if let Err(__e) = __tc_outer { ::std::panic::resume_unwind(__e); } }",
                );
                writeln!(out)?;
                return Ok(());
            }
            // PMAT-1070: `finally` wraps the whole try/except in an OUTER
            // catch_unwind so it runs in EVERY exit path.
            let has_finally = !finally.is_empty();
            let bind = |out: &mut String, name: &str| -> Result<(), RuchyCodegenError> {
                write!(
                    out,
                    "let {name} = __xpile_m.strip_prefix(\"xpile: \").and_then(|__s| __s.splitn(2, \": \").nth(1)).unwrap_or(__xpile_m).to_string(); "
                )?;
                Ok(())
            };
            if has_finally {
                write!(
                    out,
                    "{indent}{{ let __tc_outer = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {{ match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {{ "
                )?;
            } else {
                write!(
                    out,
                    "{indent}match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {{ "
                )?;
            }
            for st in body {
                emit_stmt_indented(out, st, "", mode)?;
            }
            out.push_str(" })) { Ok(_) => {}, ");
            if extra_handlers.is_empty() {
                if except_types.is_empty() {
                    // PMAT-1105 (c): gate the catch-all — Python exceptions
                    // only; a capability/honesty refusal re-raises (mirrors
                    // the Rust backend).
                    write!(out, "Err(__xpile_e) => {{ let __xpile_m: &str = __xpile_e.downcast_ref::<String>().map(|__s| __s.as_str()).or_else(|| __xpile_e.downcast_ref::<&str>().copied()).unwrap_or(\"\"); if {IS_PY_EXC_PRED} {{ ")?;
                    if let Some(name) = bound_name {
                        bind(out, name)?;
                    }
                    for st in handler {
                        emit_stmt_indented(out, st, "", mode)?;
                    }
                    out.push_str(" } else { ::std::panic::resume_unwind(__xpile_e) } }");
                } else {
                    out.push_str("Err(__xpile_e) => { let __xpile_m: &str = __xpile_e.downcast_ref::<String>().map(|__s| __s.as_str()).or_else(|| __xpile_e.downcast_ref::<&str>().copied()).unwrap_or(\"\"); if ");
                    for (i, k) in except_types.iter().enumerate() {
                        if i > 0 {
                            out.push_str(" || ");
                        }
                        write_exc_tag_pred(out, k)?;
                    }
                    out.push_str(" { ");
                    if let Some(name) = bound_name {
                        bind(out, name)?;
                    }
                    for st in handler {
                        emit_stmt_indented(out, st, "", mode)?;
                    }
                    out.push_str(" } else { ::std::panic::resume_unwind(__xpile_e) } }");
                }
            } else {
                // PMAT-1059: multiple `except` clauses — an ordered
                // if/else-if chain over [first] ++ extra_handlers; the chain
                // ends in `resume_unwind` (an unmatched payload PROPAGATES).
                // PMAT-1082: a catch-all (empty types) in NON-final position
                // still terminates the chain and DROPS the later arms
                // (unreachable in CPython too). PMAT-1105: the catch-all arm
                // is GATED (Python exceptions only — a refusal re-raises via
                // the trailing `else`, now always emitted), and `except
                // Exception:` is an ordinary discriminated arm via the
                // frontend's sentinel tag (mirrors the Rust backend).
                out.push_str("Err(__xpile_e) => { let __xpile_m: &str = __xpile_e.downcast_ref::<String>().map(|__s| __s.as_str()).or_else(|| __xpile_e.downcast_ref::<&str>().copied()).unwrap_or(\"\"); ");
                let all: Vec<(&Vec<String>, &Option<String>, &Vec<Stmt>)> =
                    std::iter::once((except_types, bound_name, handler))
                        .chain(
                            extra_handlers
                                .iter()
                                .map(|h| (&h.except_types, &h.bound_name, &h.body)),
                        )
                        .collect();
                let mut catch_all_seen = false;
                for (i, (types, name, hbody)) in all.iter().enumerate() {
                    if i > 0 {
                        out.push_str(" else ");
                    }
                    if types.is_empty() {
                        write!(out, "if {IS_PY_EXC_PRED} {{ ")?;
                        catch_all_seen = true;
                    } else {
                        out.push_str("if ");
                        for (j, k) in types.iter().enumerate() {
                            if j > 0 {
                                out.push_str(" || ");
                            }
                            write_exc_tag_pred(out, k)?;
                        }
                        out.push_str(" { ");
                    }
                    if let Some(n) = name {
                        bind(out, n)?;
                    }
                    for st in hbody.iter() {
                        emit_stmt_indented(out, st, "", mode)?;
                    }
                    out.push_str(" }");
                    if catch_all_seen {
                        break;
                    }
                }
                out.push_str(" else { ::std::panic::resume_unwind(__xpile_e) }");
                out.push_str(" }");
            }
            out.push_str(" }");
            if has_finally {
                out.push_str(" })); ");
                for st in finally {
                    emit_stmt_indented(out, st, "", mode)?;
                }
                out.push_str(
                    "if let Err(__e) = __tc_outer { ::std::panic::resume_unwind(__e); } }",
                );
            }
            writeln!(out)?;
            Ok(())
        }
        // PMAT-039 / XPILE-BASHRS-MERGER-001 Layer B: see rust-codegen's
        // matching arm. Ruchy compiles to Rust and inherits Rust's
        // disposition — no Ruchy-level translation of `Stmt::Cmd`
        // exists.
        Stmt::FileWrite {
            path,
            content,
            append,
        } => {
            if *append {
                // PMAT-1078: `open(p, "a").write(s)` — append (create if absent),
                // via OpenOptions + write_all (std::fs::write only truncates).
                write!(out, "{indent}{{ use ::std::io::Write as _; let mut __wf = ::std::fs::OpenOptions::new().create(true).append(true).open(&(")?;
                emit_expr(out, path, mode)?;
                out.push_str(r##")).unwrap_or_else(|__e| if __e.kind() == ::std::io::ErrorKind::NotFound { panic!("xpile: FileNotFoundError: {}", __e) } else if __e.kind() == ::std::io::ErrorKind::PermissionDenied { panic!("xpile: PermissionError: {}", __e) } else { panic!("xpile: OSError: {}", __e) }); __wf.write_all((&("##);
                emit_expr(out, content, mode)?;
                out.push_str(r##")).as_bytes()).unwrap_or_else(|__e| panic!("xpile: OSError: {}", __e)); }"##);
                writeln!(out)?;
                return Ok(());
            }
            // PMAT-1075: `open(p, "w").write(s)` → inline std::fs::write (truncate).
            // Borrow path + content (`&(...)`) via AsRef so a variable path/content
            // isn't moved (it may be read again after the write) — E0382 otherwise.
            write!(out, "{indent}::std::fs::write(&(")?;
            emit_expr(out, path, mode)?;
            out.push_str("), &(");
            emit_expr(out, content, mode)?;
            out.push_str(r##")).unwrap_or_else(|__e| if __e.kind() == ::std::io::ErrorKind::NotFound { panic!("xpile: FileNotFoundError: {}", __e) } else if __e.kind() == ::std::io::ErrorKind::PermissionDenied { panic!("xpile: PermissionError: {}", __e) } else { panic!("xpile: OSError: {}", __e) });"##);
            writeln!(out)?;
            Ok(())
        }
        Stmt::Cmd { program, args } => Err(RuchyCodegenError::Unsupported(format!(
            "Ruchy backend does not lower Stmt::Cmd (`{program}` with {} arg(s)) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs this construct; \
             use `--target shell` to emit POSIX sh via bashrs-backend",
            args.len()
        ))),
        // PMAT-041: same disposition as Cmd.
        Stmt::Pipeline { stages } => Err(RuchyCodegenError::Unsupported(format!(
            "Ruchy backend does not lower Stmt::Pipeline ({} stages) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell pipelines; \
             use `--target shell`",
            stages.len()
        ))),
        // PMAT-048: same disposition.
        Stmt::ShellLoop { .. } => Err(RuchyCodegenError::Unsupported(
            "Ruchy backend does not lower Stmt::ShellLoop — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell loops; \
             use `--target shell`"
                .into(),
        )),
        // PMAT-051: same disposition.
        Stmt::ShellAssign { name, .. } => Err(RuchyCodegenError::Unsupported(format!(
            "Ruchy backend does not lower Stmt::ShellAssign (`{name}=…`) — \
             contract C-BASHRS-POSIX-IDEMPOTENCE governs shell variable assignment; \
             use `--target shell`"
        ))),
    }
}

fn emit_param(out: &mut String, p: &Param) -> Result<(), RuchyCodegenError> {
    // PMAT-506d: a method's `self` receiver emits as `&self`.
    if p.name == "self" {
        out.push_str("&self");
        return Ok(());
    }
    // PMAT-460: same posture as the Rust backend.
    if p.mutable {
        write!(out, "mut ")?;
    }
    write!(out, "{}: ", p.name)?;
    emit_type(out, &p.ty)?;
    Ok(())
}

/// Escape a string for emission inside a Ruchy `"..."` literal.
/// PMAT-449 — Ruchy compiles to Rust, so identical escape semantics.
/// PMAT-748 (HUNT-V14 #3): escape control bytes when emitting a `str` literal
/// (mirror of the rust backend). A bare CR is a rustc error and a raw CRLF is
/// normalized to LF (CR silently dropped); escape the common control chars by
/// name and every other C0/DEL char via `\u{..}`.
fn escape_ruchy_str(s: &str) -> String {
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

fn emit_type(out: &mut String, t: &Type) -> Result<(), RuchyCodegenError> {
    match t {
        Type::I64 => out.push_str("i64"),
        // PMAT-909: a C `long`/`int64_t` (distinct 64-bit-ABI width) → `i64`
        // (Ruchy compiles to Rust; value-compatible with I64).
        Type::CLong => out.push_str("i64"),
        // PMAT-918: a C `unsigned`/`uint32_t` (distinct 32-bit UNSIGNED width)
        // → `u32` (Ruchy compiles to Rust; the C ABI distinction lives in
        // `c_abi_type` -> c_uint).
        Type::CUInt => out.push_str("u32"),
        // PMAT-921: a C `unsigned long`/`uint64_t` (distinct 64-bit UNSIGNED
        // width) → `u64` (Ruchy compiles to Rust; the C ABI distinction lives
        // in `c_abi_type` -> c_ulonglong).
        Type::CULong => out.push_str("u64"),
        // PMAT-477 (R8): Ruchy → Rust `f64`.
        Type::F64 => out.push_str("f64"),
        // PMAT-911: a C `float` (distinct 32-bit-ABI width) → `f32` (Ruchy
        // compiles to Rust; the C ABI distinction lives in `c_abi_type`).
        Type::F32 => out.push_str("f32"),
        Type::Bool => out.push_str("bool"),
        // PMAT-502bl: Python `None` return → unit `()`.
        Type::Unit => out.push_str("()"),
        // Ruchy compiles to Rust → same BigInt re-export. PMAT-012.
        Type::BigInt => out.push_str("xpile_bigint::BigInt"),
        // PMAT-449 (v0.2.0 Track 1.A): Ruchy → Rust → owned `String`,
        // mirrors xpile-rust-codegen's lowering.
        Type::Str => out.push_str("String"),
        // PMAT-455 (v0.2.0 Track 1.B): Ruchy → Rust Vec<T>.
        Type::List(elem_ty) => {
            out.push_str("Vec<");
            emit_type(out, elem_ty)?;
            out.push('>');
        }
        // PMAT-462 (v0.2.0 Track 1.C): Ruchy → Rust HashMap<K, V>.
        Type::Dict(k_ty, v_ty) => {
            out.push_str("indexmap::IndexMap<");
            emit_type(out, k_ty)?;
            out.push_str(", ");
            emit_type(out, v_ty)?;
            out.push('>');
        }
        // PMAT-500: Ruchy → Rust `HashSet<T>`.
        Type::Set(elem_ty) => {
            out.push_str("std::collections::HashSet<");
            emit_type(out, elem_ty)?;
            out.push('>');
        }
        // PMAT-494: Ruchy → Rust `(T0, T1, ...)`.
        Type::Tuple(elems) => {
            out.push('(');
            for (i, t) in elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_type(out, t)?;
            }
            // PMAT-625: 1-element tuple needs `(T,)` (matches the Rust backend).
            if elems.len() == 1 {
                out.push(',');
            }
            out.push(')');
        }
        // PMAT-046: same disposition as the Rust backend.
        Type::ShellString | Type::ExitCode => {
            return Err(RuchyCodegenError::Unsupported(format!(
                "Ruchy backend does not lower {t:?} — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs the bashrs type domain; \
                 use `--target shell`"
            )));
        }
        // PMAT-502ew: Python `Optional[T]` → `Option<T>`, matching Rust.
        Type::Optional(inner) => {
            out.push_str("Option<");
            emit_type(out, inner)?;
            out.push('>');
        }
        // PMAT-506b: struct-typed value emits the bare struct name.
        Type::Struct(name) => out.push_str(name),
        // PMAT-924: a C `char` (8-bit) → `i8` (Ruchy compiles to Rust; matches
        // the Rust backend). Pointer-pointee only.
        Type::CChar => out.push_str("i8"),
        // PMAT-924: a C pointer `T*` → a raw Rust pointer `*mut`/`*const
        // <pointee>` (Ruchy compiles to Rust; the ABI-honest FFI rendering lives
        // in xpile-ffi-manifest's `c_abi_render`).
        Type::Ptr { mutable, pointee } => {
            out.push_str(if *mutable { "*mut " } else { "*const " });
            emit_type(out, pointee)?;
        }
    }
    Ok(())
}

/// PMAT-560: does `e` reference the identifier `name`? (See the Rust backend's
/// twin for the `IndexAssign` self-referential-index rationale.)
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
        _ => false,
    }
}

/// PMAT-730/PMAT-1037: the shared writer for a subscript store through `base`
/// — [`Stmt::NestedSubscriptAssign`] (plain-Name base, `len >= 2`) and
/// [`Stmt::FieldIndexAssign`] (`<obj>.<field>` base, `len >= 1`) emit the same
/// shape. PMAT-833 (HUNT-V26 #3): bind the RHS BEFORE `&mut base` so a nested
/// read-modify-write (`d["a"]["x"] = d["a"]["x"] + 5`) doesn't read `base`
/// immutably under the live `&mut base` borrow (E0502). Mirrors the Rust
/// backend.
fn emit_subscript_write_through(
    out: &mut String,
    indent: &str,
    base: &str,
    steps: &[(Expr, bool)],
    value: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    let n = steps.len();
    // PMAT-1049: bind the RHS and then EVERY step index to an OWNED temp
    // BEFORE `&mut base`. An index that itself borrows `base` —
    // `self.xs[self.pop()]` (pop needs &mut self.xs), `self.xs[self.next_slot()]`
    // (a &mut self method) — must be evaluated before the mutable base borrow,
    // else rustc E0499/E0502. The temp is `.clone()`d (every xpile index/key
    // type is `Clone`; a Copy index's clone is a no-op) so a non-Copy (str)
    // key is NOT moved out of a caller binding reused in a later store
    // (`self.cells[r] = {}` then `self.cells[r][c] = v` — PMAT-1045). RHS-first
    // preserves Python value-before-target order (PMAT-833); the base is a
    // plain place, so evaluating indices before it is observably identical.
    write!(out, "{indent}{{ let __rhs = ")?;
    emit_expr(out, value, mode)?;
    out.push_str("; ");
    for (i, (idx, _)) in steps.iter().enumerate() {
        write!(out, "let __sidx{i} = (")?;
        emit_expr(out, idx, mode)?;
        out.push_str(").clone(); ");
    }
    write!(out, "let __t0 = &mut {base}; ")?;
    for (i, (_, is_dict)) in steps[..n - 1].iter().enumerate() {
        if *is_dict {
            write!(
                out,
                "let __t{} = __t{i}.get_mut(&__sidx{i}).unwrap(); ",
                i + 1
            )?;
        } else {
            write!(out, "let __li{i} = __sidx{i} as i64; let __lx{i} = if __li{i} < 0 {{ __t{i}.len() as i64 + __li{i} }} else {{ __li{i} }}; let __t{} = &mut __t{i}[__lx{i} as usize]; ", i + 1)?;
        }
    }
    let (_, leaf_is_dict) = &steps[n - 1];
    if *leaf_is_dict {
        // The owned leaf key temp moves into `.insert` (already an owned clone).
        write!(out, "__t{}.insert(__sidx{}, __rhs); }}", n - 1, n - 1)?;
    } else {
        write!(out, "let __ll = __sidx{} as i64; let __lx = if __ll < 0 {{ __t{}.len() as i64 + __ll }} else {{ __ll }}; __t{}[__lx as usize] = __rhs; }}", n - 1, n - 1, n - 1)?;
    }
    writeln!(out)?;
    Ok(())
}

fn emit_expr(out: &mut String, e: &Expr, mode: bool) -> Result<(), RuchyCodegenError> {
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
            // PMAT-025: in BigInt mode, append `.clone()` to every
            // Ident reference. BigInt isn't `Copy` (it's
            // heap-allocated), so a name referenced in cond +
            // branches + recursive call would move-on-first-use.
            // Mirrors the Rust backend's PMAT-013 emission.
            if mode {
                write!(out, "{}.clone()", name)?;
            } else {
                write!(out, "{}", name)?;
            }
        }
        Expr::LitInt(v) => {
            if mode {
                write!(out, "xpile_bigint::BigInt::from({}i64)", v)?;
            } else {
                write!(out, "{}i64", v)?;
            }
        }
        // PMAT-477 (R8): float literal + plain-infix float arithmetic.
        Expr::LitFloat(v) => {
            // PMAT-866 (HUNT-V30 #17): non-finite float literal → f64 constant
            // (mirror the Rust backend; `{}f64` emitted invalid `inff64`).
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
            // PMAT-614: Python float floor-division is CPython `float_divmod`,
            // not `(a / b).floor()` (the naive floor over-rounds `1.0 // 0.1` to
            // 10.0 vs Python's 9.0, and mishandles infinite operands). Matches
            // the Rust backend: fmod-based div with sign-adjust + round-up.
            // PMAT-581: guard the zero divisor (Python raises ZeroDivisionError).
            FloatOp::FloorDiv => {
                out.push_str("{ let __fa: f64 = ");
                emit_expr(out, lhs, mode)?;
                out.push_str("; let __fz: f64 = ");
                emit_expr(out, rhs, mode)?;
                // PMAT-651: zero-quotient case returns copysign(0.0, a/b) like
                // CPython's float_divmod, preserving the sign of a zero result
                // (`-0.0 // 1.0` → `-0.0`). See the rust backend.
                out.push_str("; if __fz == 0.0 { panic!(\"xpile: ZeroDivisionError: float floor division by zero\"); } let __fm = __fa % __fz; let mut __fd = (__fa - __fm) / __fz; if __fm != 0.0 && ((__fz < 0.0) != (__fm < 0.0)) { __fd -= 1.0; } if __fd != 0.0 { let __ffl = __fd.floor(); if __fd - __ffl > 0.5 { __ffl + 1.0 } else { __ffl } } else { (0.0_f64).copysign(__fa / __fz) } }");
            }
            // PMAT-591: Python float modulo is CPython `float_rem` —
            // `fmod(a,b)` (Rust `%`) + sign-adjust toward the divisor, else
            // `copysign(0.0,b)`. Matches the Rust backend (the prior floor
            // formula diverged in the last ULP and lost the signed zero).
            // PMAT-581: guard the zero divisor; bind operands (evaluate-once).
            FloatOp::Mod => {
                out.push_str("{ let __fz: f64 = ");
                emit_expr(out, rhs, mode)?;
                // PMAT-862 (HUNT-V29 #9): CPython says "float modulo by zero".
                out.push_str("; if __fz == 0.0 { panic!(\"xpile: ZeroDivisionError: float modulo by zero\"); } let __fn: f64 = ");
                emit_expr(out, lhs, mode)?;
                out.push_str("; let __r = __fn % __fz; if __r != 0.0 { if (__fz < 0.0) != (__r < 0.0) { __r + __fz } else { __r } } else { 0.0_f64.copysign(__fz) } }");
            }
            // PMAT-502bt/em/en: method-style float ops — `(a).<method>(b)`,
            // matching the Rust backend.
            // PMAT-734b (HUNT-V11 V11-10): float `**` overflow → OverflowError /
            // `0.0 ** <neg>` → ZeroDivisionError (mirrors the Rust backend).
            FloatOp::Pow => {
                out.push_str("{ let __pb: f64 = ");
                emit_expr(out, lhs, mode)?;
                out.push_str("; let __pe: f64 = ");
                emit_expr(out, rhs, mode)?;
                out.push_str("; let __pr = __pb.powf(__pe); if __pr.is_infinite() && __pb.is_finite() { if __pb == 0.0 { panic!(\"xpile: ZeroDivisionError: 0.0 cannot be raised to a negative power\"); } panic!(\"xpile: OverflowError: (34, 'Numerical result out of range')\"); } __pr }");
            }
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
            // PMAT-581: float `/` (and int true-division) raises ZeroDivisionError.
            FloatOp::Div => {
                // PMAT-862 (HUNT-V29 #9): int/int true division → "division by
                // zero"; only a float operand → "float division by zero".
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
        // PMAT-456 (v0.2.0 Track 1.B): Ruchy → Rust → lowercase
        // `true` / `false`.
        Expr::LitBool(b) => write!(out, "{}", b)?,
        Expr::BinOp { op, lhs, rhs } => emit_binop(out, *op, lhs, rhs, mode)?,
        // PMAT-451 (v0.2.0 Track 1.A): same str-concat shape as the
        // Rust backend — Ruchy compiles to Rust, so `format!()` works
        // identically.
        Expr::Concat { lhs, rhs } => {
            out.push_str("format!(\"{}{}\", ");
            emit_expr(out, lhs, mode)?;
            out.push_str(", ");
            emit_expr(out, rhs, mode)?;
            out.push(')');
        }
        // PMAT-502bg: `xs + ys` (lists), matching the Rust backend.
        Expr::ListConcat { lhs, rhs } => {
            out.push('(');
            emit_expr(out, lhs, mode)?;
            out.push_str(").iter().chain((");
            emit_expr(out, rhs, mode)?;
            out.push_str(").iter()).cloned().collect::<Vec<_>>()");
        }
        // PMAT-502bh: `"<fmt>".format(args…)`, matching the Rust backend.
        Expr::StrFormat { fmt, args } => {
            write!(out, "format!({fmt:?}")?;
            for a in args {
                out.push_str(", ");
                emit_expr(out, a, mode)?;
            }
            out.push(')');
        }
        // PMAT-502am: a formatted f-string field → `format!("{:<spec>}", v)`.
        Expr::FormatSpec {
            value,
            rust_spec,
            of_float,
        } => {
            // PMAT-659: NaN prints "nan" in Python, "NaN" in Rust — guard a BARE
            // float-precision spec (`.<digit>`, no width → unpadded "nan" matches
            // Python). See rust backend. (Width+precision NaN deferred.)
            // PMAT-947: gate on `of_float` so a str-precision `.N` (truncate, value
            // is a `String`) takes the plain `format!` branch (no `.is_nan()`).
            let bare = rust_spec.strip_prefix('+').unwrap_or(rust_spec).as_bytes();
            let is_float_prec = *of_float
                && bare.first() == Some(&b'.')
                && bare.get(1).is_some_and(|b| b.is_ascii_digit());
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
        // PMAT-502cd: `s[i]` over a string (see the Rust backend's twin) —
        // materialise the chars and index them (negative counts from the end).
        Expr::StrCharAt { string, index } => {
            // PMAT-801 (HUNT-V19 STR-IDX-OOB): tag the out-of-range panic
            // `xpile: IndexError:` so a typed except catches it (mirror Rust backend).
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
        // PMAT-702: assert exactly one char (Python `ord("ab")` is a TypeError,
        // not the first char). Mirrors the rust backend.
        Expr::Ord { value } => {
            // PMAT-725 (HUNT-V10 V10-2): bind the operand in `let __os = &(...)`
            // before `.chars()` to avoid E0716 on `ord(s[0])` (mirrors rust backend).
            out.push_str("({ let __os = &(");
            emit_expr(out, value, mode)?;
            out.push_str(
                "); let mut __oc = __os.chars(); let __c0 = __oc.next().expect(\"xpile: ord() expected a character, got an empty string (TypeError)\"); if __oc.next().is_some() { panic!(\"xpile: ord() expected a character (TypeError)\"); } __c0 as i64 })",
            );
        }
        // PMAT-1096: range-check before the cast — OverflowError outside C-int,
        // ValueError outside range(0x110000), honest UNTYPED surrogate panic
        // (CPython succeeds there; Rust `char` can't). Mirrors the rust backend.
        Expr::Chr { value } => {
            out.push_str("({ let __chn: i64 = (");
            emit_expr(out, value, mode)?;
            out.push_str(
                "); if __chn < -2147483648 || __chn > 2147483647 { panic!(\"xpile: OverflowError: Python int too large to convert to C int\"); } if __chn < 0 || __chn > 1114111 { panic!(\"xpile: ValueError: chr() arg not in range(0x110000)\"); } if (55296..=57343).contains(&__chn) { panic!(\"xpile: chr({}): surrogate code point (U+D800..U+DFFF) is unrepresentable as a Rust char — CPython chr() succeeds here; ruchy-lane limitation\", __chn); } char::from_u32(__chn as u32).expect(\"unreachable: chr() operand range-checked\").to_string() })",
            );
        }
        // PMAT-502cv: hex/oct/bin → radix string (see the Rust backend).
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
            // PMAT-923: Python's `#X` alt-form uppercases the prefix letter too
            // (`"0XFF"`); the upper-hex prefix is `0X` (mirror of the Rust
            // backend). Only `%X` (prefix suppressed) and `#X` reach upper-hex,
            // so the `hex()` builtin (`upper: false`) is unaffected.
            let (prefix, spec) = match radix {
                Radix::Hex if *upper => ("0X", "{:X}"),
                Radix::Hex => ("0x", "{:x}"),
                Radix::Oct => ("0o", "{:o}"),
                Radix::Bin => ("0b", "{:b}"),
            };
            let pfx = if *prefixed { prefix } else { "" };
            if *min_width == 0 {
                write!(out, "format!(\"{{}}{pfx}{spec}\", __sign, __m) }}")?;
            } else {
                // PMAT-773: sign-aware zero-pad (mirror of the Rust backend).
                write!(
                    out,
                    "let __body = format!(\"{spec}\", __m); let __pad = ({min_width}usize).saturating_sub(__sign.len() + {pfx_len}); format!(\"{{0}}{pfx}{{1:0>2$}}\", __sign, __body, __pad) }}",
                    pfx_len = pfx.len()
                )?;
            }
        }
        // PMAT-939 (correctness-hunt): thousands-grouping `f"{n:,}"` / `f"{n:_}"`
        // — the digit-grouping loop (mirror of the Rust backend). Rust/Ruchy
        // `format!` has no grouping flag; group the magnitude's decimal digits by
        // 3 from the right, sign first (`__m = n.unsigned_abs()` is `i64::MIN`-safe).
        Expr::IntGroupedStr { value, sep } => {
            out.push_str("{ let __n = (");
            emit_expr(out, value, mode)?;
            out.push_str(
                "); let __m = __n.unsigned_abs(); let __sign = if __n < 0 { \"-\" } else { \"\" }; \
                 let __ds = __m.to_string(); let __bytes = __ds.as_bytes(); let __len = __bytes.len(); \
                 let mut __g = String::new(); for (__i, __ch) in __bytes.iter().enumerate() { ",
            );
            write!(
                out,
                "if __i > 0 && (__len - __i) % 3 == 0 {{ __g.push('{sep}'); }} "
            )?;
            out.push_str("__g.push(*__ch as char); } format!(\"{}{}\", __sign, __g) }");
        }
        // PMAT-940 (correctness-hunt): thousands-grouped FLOAT with a fixed
        // precision `f"{x:,.Nf}"` / `f"{x:_.Nf}"` (`,f` defaults to 6 decimals) —
        // mirror of the Rust backend. Render to `precision` decimals, split off the
        // sign and the `.dd` tail, group only the integer-part digits by 3, then
        // reassemble sign + group + fraction.
        Expr::FloatGroupedStr {
            value,
            sep,
            precision,
        } => {
            out.push_str("{ let __x = (");
            emit_expr(out, value, mode)?;
            out.push_str("); let __s = ");
            match precision {
                // PMAT-940: fixed-precision render (`:,.Nf`).
                Some(p) => {
                    write!(out, "format!(\"{{:.{p}}}\", __x)")?;
                }
                // PMAT-982: bare `:,` / `:_` over the DEFAULT float repr — render
                // via the shared `str(float)` repr block (mirror of the Rust twin).
                None => {
                    out.push_str(&py_float_repr_block("__x"));
                }
            }
            // Group the LEADING integer-part digit run by 3 from the right; leave
            // the rest (`.dd`, `e+16`, or empty for `inf`/`nan`) intact. Mirror of
            // the Rust backend (Ruchy compiles to Rust).
            out.push_str(
                "; let __neg = __s.starts_with('-'); \
                 let __body = if __neg { &__s[1..] } else { &__s[..] }; \
                 let __ip = __body.find(|__c: char| !__c.is_ascii_digit()).unwrap_or(__body.len()); \
                 let (__int, __rest) = __body.split_at(__ip); \
                 let __bytes = __int.as_bytes(); let __len = __bytes.len(); \
                 let mut __g = String::new(); for (__i, __ch) in __bytes.iter().enumerate() { ",
            );
            write!(
                out,
                "if __i > 0 && (__len - __i) % 3 == 0 {{ __g.push('{sep}'); }} "
            )?;
            out.push_str(
                "__g.push(*__ch as char); } \
                 format!(\"{}{}{}\", if __neg { \"-\" } else { \"\" }, __g, __rest) }",
            );
        }
        // PMAT-941 (correctness-hunt): scientific-notation float `f"{x:e}"` /
        // `f"{x:.NE}"` — mirror of the Rust backend. Render to `precision` decimals,
        // then fix up the exponent to Python's `e±NN` form (sign + 2-digit-min
        // zero-pad) and case-fold the non-finite inf/nan tail.
        Expr::FloatSciStr {
            value,
            precision,
            upper,
        } => {
            let echar = if *upper { 'E' } else { 'e' };
            let fold = if *upper {
                "to_uppercase"
            } else {
                "to_lowercase"
            };
            out.push_str("{ let __x = (");
            emit_expr(out, value, mode)?;
            out.push_str("); let __s = ");
            write!(out, "format!(\"{{:.{precision}{echar}}}\", __x)")?;
            out.push_str(
                "; match __s.split_once(['e', 'E']) { \
                 Some((__mant, __exp)) => { \
                 let __ev: i64 = __exp.parse().expect(\"xpile: scientific exponent\"); ",
            );
            write!(
                out,
                "format!(\"{{}}{echar}{{}}{{:02}}\", __mant, \
                 if __ev < 0 {{ '-' }} else {{ '+' }}, __ev.abs()) }}, "
            )?;
            write!(out, "None => __s.{fold}() }} }}")?;
        }
        // PMAT-965 (correctness-hunt): the GENERAL-float spec `f"{x:g}"` / `f"{x:.NG}"`
        // — mirror of the Rust backend (Ruchy compiles to Rust). Port the C `%g`
        // rule: pick fixed or scientific by the decimal exponent, strip trailing
        // zeros, fix the exponent to Python's `e±NN`. inf/nan pass through (upper
        // → INF/NAN).
        Expr::FloatGeneralStr {
            value,
            precision,
            upper,
        } => {
            let prec = (*precision).max(1);
            let prec_m1 = prec - 1;
            out.push_str("{ let __x: f64 = (");
            emit_expr(out, value, mode)?;
            out.push_str("); ");
            if *upper {
                out.push_str(
                    "if __x.is_nan() { \"NAN\".to_string() } \
                     else if __x.is_infinite() { if __x < 0.0 { \"-INF\".to_string() } else { \"INF\".to_string() } } ",
                );
            } else {
                out.push_str(
                    "if __x.is_nan() { \"nan\".to_string() } \
                     else if __x.is_infinite() { if __x < 0.0 { \"-inf\".to_string() } else { \"inf\".to_string() } } ",
                );
            }
            out.push_str("else { ");
            write!(
                out,
                "let __sci = format!(\"{{:.{prec_m1}e}}\", __x); \
                 let __xe: i64 = __sci.split_once('e').map(|(_, __e)| __e.parse().expect(\"xpile: general-float exponent\")).expect(\"xpile: general-float exponent\"); "
            )?;
            write!(
                out,
                "let __out = if (-4..{prec_i}).contains(&__xe) {{ \
                 let __p = ({prec_i} - 1 - __xe).max(0) as usize; format!(\"{{:.*}}\", __p, __x) }} \
                 else {{ format!(\"{{:.{prec_m1}e}}\", __x) }}; ",
                prec_i = prec as i64
            )?;
            out.push_str(
                "let __res = if let Some((__m, __e)) = __out.split_once('e') { \
                 let __mt = if __m.contains('.') { __m.trim_end_matches('0').trim_end_matches('.') } else { __m }; \
                 let __ev: i64 = __e.parse().expect(\"xpile: general-float exponent\"); \
                 format!(\"{}e{}{:02}\", __mt, if __ev < 0 { '-' } else { '+' }, __ev.abs()) } \
                 else if __out.contains('.') { __out.trim_end_matches('0').trim_end_matches('.').to_string() } \
                 else { __out }; ",
            );
            if *upper {
                out.push_str("__res.to_uppercase() } }");
            } else {
                out.push_str("__res } }");
            }
        }
        // PMAT-942 (correctness-hunt): the SPACE sign flag `f"{5: d}"` / `f"{x: .2f}"`
        // (see the rust backend). Render with the `+` spec (which composes width/
        // precision like Python) and swap the leading `+` for a space — a no-op for
        // negatives, which carry `-` not `+`.
        Expr::SpaceSignStr { value, rust_spec } => {
            write!(out, "format!(\"{{:{rust_spec}}}\", ")?;
            emit_expr(out, value, mode)?;
            out.push_str(").replacen('+', \" \", 1)");
        }
        // PMAT-502da: `int(s, base)`. PMAT-655: accept a base-matching radix
        // prefix (0x/0o/0b) + PEP-515 underscores (see the rust backend).
        Expr::IntFromStrRadix { value, radix } => {
            let radix = *radix;
            out.push_str("{ let __ri = &(");
            emit_expr(out, value, mode)?;
            out.push_str("); let __rt = __ri.trim(); let (__rsgn, __rb): (&str, &str) = match __rt.strip_prefix('-') { Some(__r) => (\"-\", __r), None => (\"\", __rt.strip_prefix('+').unwrap_or(__rt)) }; ");
            // PMAT-718 (HUNT-V9 V9-3): validate PEP-515 underscore placement before
            // the blanket `replace('_', "")` — Python raises ValueError on a leading,
            // trailing, or doubled underscore. Check runs on the post-sign, pre-prefix
            // string so a legal underscore after the base prefix (`int("0x_ff", 16)`)
            // survives. Mirrors the Rust backend.
            // PMAT-1089: both radix panics quote the ORIGINAL (untrimmed)
            // argument like CPython, and the parse failure formats via
            // `unwrap_or_else` (mirror rust).
            out.push_str(&format!(
                "if __rb.starts_with('_') || __rb.ends_with('_') || __rb.contains(\"__\") {{ panic!(\"xpile: ValueError: invalid literal for int() with base {radix}: {{}}\", {repr}); }} ",
                repr = py_str_repr_block("__ri")
            ));
            let prefix_strip = match radix {
                16 => "let __rb = __rb.strip_prefix(\"0x\").or_else(|| __rb.strip_prefix(\"0X\")).unwrap_or(__rb); ",
                8 => "let __rb = __rb.strip_prefix(\"0o\").or_else(|| __rb.strip_prefix(\"0O\")).unwrap_or(__rb); ",
                2 => "let __rb = __rb.strip_prefix(\"0b\").or_else(|| __rb.strip_prefix(\"0B\")).unwrap_or(__rb); ",
                _ => "",
            };
            out.push_str(prefix_strip);
            // PMAT-1097: three-way from_str_radix failure classification
            // (mirror rust) — well-formed base-{radix} digits that fail parse
            // are i64 overflow (CPython bigint) → honest range message;
            // all-numeric non-ASCII is CPython's Unicode-decimal acceptance →
            // honest digit-class refusal; the rest keeps the exact CPython
            // invalid-literal message.
            out.push_str(
                "let __rd = __rb.replace('_', \"\"); let __rc = format!(\"{}{}\", __rsgn, __rd); ",
            );
            out.push_str(&format!(
                "if !__rd.is_empty() && __rd.chars().all(|__c| __c.to_digit({radix}).is_some()) {{ i64::from_str_radix(&__rc, {radix}).unwrap_or_else(|_| panic!(\"xpile: int() out of i64 range; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")) }} else if !__rd.is_empty() && __rd.chars().all(|__c| __c.is_numeric()) {{ panic!(\"xpile: int() with non-ASCII digits: CPython accepts Unicode decimal digits; not yet implemented\") }} else {{ i64::from_str_radix(&__rc, {radix}).unwrap_or_else(|_| panic!(\"xpile: ValueError: invalid literal for int() with base {radix}: {{}}\", {repr})) }} }}",
                repr = py_str_repr_block("__ri")
            ));
        }
        // PMAT-492/493b: Python string methods (Ruchy → Rust). No-arg
        // transforms emit a suffix; startswith/endswith emit
        // `.starts_with(&(<pat>)[..])` (the reslice yields `&str`).
        Expr::StrMethod { recv, op, args } => {
            // PMAT-492d: `join` inverts receiver/arg (sep.join(xs) → xs.join(sep)).
            if matches!(op, StrMethodOp::Join) {
                emit_expr(out, &args[0], mode)?;
                out.push_str(".join(&(");
                emit_expr(out, recv, mode)?;
                out.push_str(")[..])");
                return Ok(());
            }
            // PMAT-695: `.isascii()` → `(s).is_ascii()` (empty → true, no guard).
            if matches!(op, StrMethodOp::IsAscii) {
                out.push('(');
                emit_expr(out, recv, mode)?;
                out.push_str(").is_ascii()");
                return Ok(());
            }
            // PMAT-502ag: `.isdigit()`/`.isalpha()`/`.isspace()` →
            // `(!(s).is_empty() && (s).chars().all(|__c| __c.<pred>()))`.
            if matches!(
                op,
                StrMethodOp::IsDigit
                    | StrMethodOp::IsNumeric
                    | StrMethodOp::IsAlpha
                    | StrMethodOp::IsSpace
                    | StrMethodOp::IsAlnum
            ) {
                out.push_str("(!(");
                emit_expr(out, recv, mode)?;
                out.push_str(").is_empty() && (");
                emit_expr(out, recv, mode)?;
                out.push_str(").chars().all(|__c| ");
                out.push_str(match op {
                    StrMethodOp::IsDigit => "__c.is_ascii_digit()",
                    // PMAT-643: Unicode Number categories (matches Python isnumeric).
                    StrMethodOp::IsNumeric => "__c.is_numeric()",
                    StrMethodOp::IsAlpha => "__c.is_alphabetic()",
                    StrMethodOp::IsAlnum => "__c.is_alphanumeric()",
                    // PMAT-600: include the C0 separators U+001C..U+001F (Python
                    // isspace whitespace set; matches the Rust backend).
                    _ => "(__c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}'))",
                });
                out.push_str("))");
                return Ok(());
            }
            // PMAT-502di: `.isupper()`/`.islower()` → cased-char predicate.
            if matches!(op, StrMethodOp::IsUpper | StrMethodOp::IsLower) {
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
                return Ok(());
            }
            // PMAT-502ah: `.capitalize()` → first char upper, rest lower.
            if matches!(op, StrMethodOp::Capitalize) {
                // PMAT-701: titlecase (not uppercase) the lead, matching Python
                // (`"ß".capitalize()` == "Ss"). std has no char::to_titlecase, so
                // keep the first char of the uppercase expansion + lowercase the
                // rest. Mirrors the rust backend.
                out.push_str("{ let __cs = &(");
                emit_expr(out, recv, mode)?;
                out.push_str("); let mut __ch = __cs.chars(); match __ch.next() { Some(__f) => { let __ue: String = __f.to_uppercase().collect(); let mut __uec = __ue.chars(); let __lead = match __uec.next() { Some(__h) => __h.to_string() + &__uec.as_str().to_lowercase(), None => String::new() }; __lead + &(__ch.as_str().to_lowercase()) }, None => String::new() } }");
                return Ok(());
            }
            // PMAT-502aj: `.title()` → title-case each word.
            if matches!(op, StrMethodOp::Title) {
                // PMAT-701: word-start titlecases via the uppercase expansion.
                out.push_str("{ let mut __tr = String::new(); let mut __pa = false; for __c in (");
                emit_expr(out, recv, mode)?;
                out.push_str(").chars() { if __c.is_alphabetic() { if __pa { __tr.extend(__c.to_lowercase()); } else { let __ue: String = __c.to_uppercase().collect(); let mut __uec = __ue.chars(); if let Some(__h) = __uec.next() { __tr.push(__h); __tr.push_str(&__uec.as_str().to_lowercase()); } } __pa = true; } else { __tr.push(__c); __pa = false; } } __tr }");
                return Ok(());
            }
            // PMAT-502aw: `.rjust(w)`/`.ljust(w)`, matching the Rust backend.
            if matches!(op, StrMethodOp::RJust | StrMethodOp::LJust) {
                let is_r = matches!(op, StrMethodOp::RJust);
                if args.len() == 2 {
                    // PMAT-632: optional fill char, matching the Rust backend.
                    out.push_str("{ let __s = (");
                    emit_expr(out, recv, mode)?;
                    out.push_str("); let __w = (");
                    emit_expr(out, &args[0], mode)?;
                    // PMAT-666: clamp negative width to 0 (see the rust backend).
                    out.push_str(").max(0) as usize; let __n = __s.chars().count(); if __n >= __w { __s } else { let __pad = (");
                    emit_expr(out, &args[1], mode)?;
                    out.push_str(").repeat(__w - __n); ");
                    out.push_str(if is_r {
                        "format!(\"{}{}\", __pad, __s) } }"
                    } else {
                        "format!(\"{}{}\", __s, __pad) } }"
                    });
                } else {
                    out.push_str(if is_r {
                        "format!(\"{:>1$}\", "
                    } else {
                        "format!(\"{:<1$}\", "
                    });
                    emit_expr(out, recv, mode)?;
                    out.push_str(", (");
                    emit_expr(out, &args[0], mode)?;
                    // PMAT-666: clamp negative width to 0 (see the rust backend).
                    out.push_str(").max(0) as usize)");
                }
                return Ok(());
            }
            // PMAT-502cq: `.removeprefix(p)`/`.removesuffix(p)` (block form,
            // matching the Rust backend).
            if matches!(op, StrMethodOp::RemovePrefix | StrMethodOp::RemoveSuffix) {
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str(if matches!(op, StrMethodOp::RemovePrefix) {
                    "); match __s.strip_prefix(&("
                } else {
                    "); match __s.strip_suffix(&("
                });
                emit_expr(out, &args[0], mode)?;
                out.push_str(")[..]) { Some(__r) => __r.to_string(), None => __s } }");
                return Ok(());
            }
            // PMAT-502cs: `.zfill(w)` (block form, matching the Rust backend).
            if matches!(op, StrMethodOp::ZFill) {
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let __w = (");
                emit_expr(out, &args[0], mode)?;
                out.push_str(").max(0) as usize; let __n = __s.chars().count(); if __n >= __w { __s } else { let __pad = \"0\".repeat(__w - __n); if __s.starts_with('-') || __s.starts_with('+') { format!(\"{}{}{}\", &__s[..1], __pad, &__s[1..]) } else { format!(\"{}{}\", __pad, __s) } } }");
                return Ok(());
            }
            // PMAT-502cu: `.center(w)` (block form, matching the Rust backend).
            if matches!(op, StrMethodOp::Center) {
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let __w = (");
                emit_expr(out, &args[0], mode)?;
                out.push_str(").max(0) as usize; let __n = __s.chars().count(); if __n >= __w { __s } else { let __marg = __w - __n; let __left = __marg / 2 + (__marg & __w & 1); ");
                if args.len() == 2 {
                    // PMAT-632: optional fill char, matching the Rust backend.
                    out.push_str("let __fc = (");
                    emit_expr(out, &args[1], mode)?;
                    out.push_str("); format!(\"{}{}{}\", __fc.repeat(__left), __s, __fc.repeat(__marg - __left)) } }");
                } else {
                    out.push_str("format!(\"{}{}{}\", \" \".repeat(__left), __s, \" \".repeat(__marg - __left)) } }");
                }
                return Ok(());
            }
            // PMAT-502dj: `.partition(sep)` / `.rpartition(sep)` → 3-tuple.
            if matches!(op, StrMethodOp::Partition | StrMethodOp::RPartition) {
                // PMAT-726 (HUNT-V10 V10-1): bind recv+sep once and guard the empty
                // separator (Python raises ValueError) — mirrors the rust backend.
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
                return Ok(());
            }
            // PMAT-502dl: `.splitlines()` → char-walk over Python's full line
            // boundary set (Rust `str::lines()` only covers LF/CRLF).
            if matches!(op, StrMethodOp::SplitLines) {
                out.push_str("{ let __s = (");
                emit_expr(out, recv, mode)?;
                out.push_str("); let mut __lines: Vec<String> = Vec::new(); let mut __cur = String::new(); let mut __it = __s.chars().peekable(); while let Some(__c) = __it.next() { match __c { '\\r' => { if __it.peek() == Some(&'\\n') { __it.next(); } __lines.push(std::mem::take(&mut __cur)); } '\\n' | '\\u{0b}' | '\\u{0c}' | '\\u{1c}' | '\\u{1d}' | '\\u{1e}' | '\\u{85}' | '\\u{2028}' | '\\u{2029}' => { __lines.push(std::mem::take(&mut __cur)); } _ => __cur.push(__c), } } if !__cur.is_empty() { __lines.push(__cur); } __lines }");
                return Ok(());
            }
            // PMAT-675: `s.find(sub, start[, end])` / `s.count(sub, start[, end])`
            // search within the char-slice `s[start:end]`; find returns the CHAR
            // index in the ORIGINAL string (or -1), count the # of non-overlapping
            // occurrences. start/end are char indices with Python clamping; end
            // defaults to len for the 2-arg form. (Mirrors the Rust backend.)
            if matches!(
                op,
                StrMethodOp::Find
                    | StrMethodOp::Count
                    | StrMethodOp::StrIndex
                    | StrMethodOp::RIndex
                    | StrMethodOp::Rfind
            ) && args.len() >= 2
            {
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
                // PMAT-854 (HUNT-V28 #11): index/rindex/rfind reuse the slice; r*
                // use rfind, *index raise ValueError (mirror the Rust backend).
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
                return Ok(());
            }
            // PMAT-691: `s.strip(chars)`/`lstrip`/`rstrip` with a char-SET arg →
            // trim_matches/start/end with a membership closure (see rust backend).
            if matches!(
                op,
                StrMethodOp::Strip | StrMethodOp::LStrip | StrMethodOp::RStrip
            ) && !args.is_empty()
            {
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
                return Ok(());
            }
            // PMAT-566: find/rfind/index/rindex return a Python CHAR index, not a
            // byte offset — bind recv to a temp and count chars before the match.
            if matches!(
                op,
                StrMethodOp::Find
                    | StrMethodOp::Rfind
                    | StrMethodOp::StrIndex
                    | StrMethodOp::RIndex
            ) {
                let finder = if matches!(op, StrMethodOp::Rfind | StrMethodOp::RIndex) {
                    "rfind"
                } else {
                    "find"
                };
                // PMAT-851 (HUNT-V28 #2): clone the receiver so `index`/`find`/…
                // don't MOVE a non-Copy String (E0382 on `i = s.index(sep); s[i:]`).
                // Mirrors the Rust backend.
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
                return Ok(());
            }
            emit_expr(out, recv, mode)?;
            match op {
                StrMethodOp::Upper => out.push_str(".to_uppercase()"),
                StrMethodOp::Lower => out.push_str(".to_lowercase()"),
                // PMAT-600: strip the Python whitespace set incl. U+001C..U+001F.
                StrMethodOp::Strip => out.push_str(".trim_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
                // PMAT-564: `len(str)` → Unicode char count (not byte len).
                StrMethodOp::CharCount => out.push_str(".chars().count() as i64"),
                // PMAT-530: `s[::-1]` → reverse by Unicode scalar value.
                StrMethodOp::Reverse => out.push_str(".chars().rev().collect::<String>()"),
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
                // PMAT-621: negative maxsplit = "no limit" — `saturating_add(1)`
                // (not `+ 1`, which wraps to 0 for a negative value). Matches Rust.
                StrMethodOp::SplitN => {
                    out.push_str(".splitn(((");
                    emit_expr(out, &args[1], mode)?;
                    out.push_str(") as usize).saturating_add(1), &(");
                    emit_expr(out, &args[0], mode)?;
                    out.push_str(")[..]).map(|__c| __c.to_string()).collect::<Vec<String>>()");
                }
                // PMAT-644: `.rsplit(sep, maxsplit)` → `.rsplitn(...)` reversed
                // (rsplitn yields right-to-left; restore Python order). Matches
                // the Rust backend.
                StrMethodOp::RSplitN => {
                    out.push_str(".rsplitn(((");
                    emit_expr(out, &args[1], mode)?;
                    out.push_str(") as usize).saturating_add(1), &(");
                    emit_expr(out, &args[0], mode)?;
                    out.push_str(")[..]).map(|__c| __c.to_string()).collect::<Vec<String>>().into_iter().rev().collect::<Vec<String>>()");
                }
                // PMAT-502co: no-arg `.split()` → whitespace split.
                // PMAT-649: include the C0 separators U+001C-1F (Python `str.split()`
                // splits on them; Rust's `split_whitespace` doesn't). Mirror the rust
                // backend — see its comment.
                StrMethodOp::SplitWhitespace => {
                    out.push_str(
                        ".split(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).filter(|__c| !__c.is_empty()).map(|__c| __c.to_string()).collect::<Vec<String>>()",
                    );
                }
                // PMAT-502b: `.replace(old, new)`.
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
                // PMAT-502l: lstrip/rstrip → trim_start/trim_end; find/count → i64.
                // PMAT-600: against the Python whitespace set (incl. U+001C..U+001F).
                StrMethodOp::LStrip => out.push_str(".trim_start_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
                StrMethodOp::RStrip => out.push_str(".trim_end_matches(|__c: char| __c.is_whitespace() || matches!(__c, '\\u{1c}'..='\\u{1f}')).to_string()"),
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
        // PMAT-455 (v0.2.0 Track 1.B): Ruchy → Rust → `vec![...]`.
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
        // PMAT-494: Python tuple literal → Ruchy → Rust `(e0, e1, ...)`.
        Expr::TupleLit(elems) => {
            out.push('(');
            for (i, e) in elems.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                emit_expr(out, e, mode)?;
            }
            // PMAT-625: 1-element tuple literal needs `(x,)` (matches Rust backend).
            if elems.len() == 1 {
                out.push(',');
            }
            out.push(')');
        }
        // PMAT-502q: Python `t[N]` (tuple) → `(<tuple>).N.clone()`.
        Expr::TupleIndex { tuple, index } => {
            out.push('(');
            emit_expr(out, tuple, mode)?;
            write!(out, ").{index}.clone()")?;
        }
        // PMAT-496/539: Python slice with full bound semantics (negative
        // bounds count from the end, clamp to `[0, len]`, `lo > hi` → empty).
        // Mirrors the Rust backend.
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
             -> Result<(), RuchyCodegenError> {
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
            // PMAT-567: str slices index by Unicode chars (collect to Vec<char>);
            // list slices keep the by-reference element-indexed &Vec. Mirrors the
            // Rust backend.
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
                // PMAT-548/633: negative step `xs[::-k]`/`s[::-k]` reverses then
                // steps; str collects into String (Vec<char>), list into Vec.
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
                // PMAT-633: positive step — str into String, list into Vec.
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
                    // PMAT-567: `__sl` is `Vec<char>` for str.
                    "__sl[__lo..__hi].iter().collect::<String>() }"
                } else {
                    "__sl[__lo..__hi].to_vec() }"
                }),
            }
        }
        // PMAT-498: scalar numeric builtins → receiver-method form.
        Expr::NumBuiltin { op, args, of_float } => {
            // PMAT-601: float max/min use Python first-arg-wins semantics
            // (matches the Rust backend); integer min/max keep `.min`/`.max`.
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
            // PMAT-606: math.floor/ceil/trunc guard the rounded value (finite +
            // i64 range) and fail loud, like the int(float) guard (Rust twin).
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
            // PMAT-794 (HUNT-V18 EXC-003): sqrt(neg)/log*(non-positive) raise
            // ValueError("math domain error") in Python; Rust returns NaN/-inf
            // silently. Guard + tagged panic (mirror of the Rust backend).
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
                // PMAT-579: checked i64 abs (see Rust twin); f64 abs is exact.
                NumBuiltinOp::Abs if *of_float => out.push_str(".abs()"),
                NumBuiltinOp::Abs => out.push_str(
                    ".checked_abs().expect(\"xpile: i64 abs overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")",
                ),
                // PMAT-502ek: math functions, matching the Rust backend.
                NumBuiltinOp::Sqrt => out.push_str(".sqrt()"),
                NumBuiltinOp::Floor => out.push_str(".floor() as i64"),
                NumBuiltinOp::Ceil => out.push_str(".ceil() as i64"),
                // PMAT-502em: `math.trunc`, matching the Rust backend.
                NumBuiltinOp::Trunc => out.push_str(".trunc() as i64"),
                // PMAT-502el: trig / exp / log — matching the Rust backend.
                NumBuiltinOp::Sin => out.push_str(".sin()"),
                NumBuiltinOp::Cos => out.push_str(".cos()"),
                NumBuiltinOp::Tan => out.push_str(".tan()"),
                NumBuiltinOp::Exp => out.push_str(".exp()"),
                NumBuiltinOp::Ln => out.push_str(".ln()"),
                NumBuiltinOp::Log10 => out.push_str(".log10()"),
                NumBuiltinOp::Log2 => out.push_str(".log2()"),
                NumBuiltinOp::Min | NumBuiltinOp::Max => {
                    // PMAT-502cz: variadic — chain over every remaining arg.
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
            // PMAT-584: CPython float sum() is Neumaier-compensated (see Rust
            // twin); int stays exact `.iter().sum::<i64>()`.
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
                // PMAT-679: skip compensation on a non-finite running total
                // (`inf - inf = NaN` would poison the result; Python yields inf).
                out.push_str(").iter() { let __st = __ss + __sx; if __st.is_finite() { if __ss.abs() >= __sx.abs() { __sc += (__ss - __st) + __sx; } else { __sc += (__sx - __st) + __ss; } } else { __sc = 0.0f64; } __ss = __st; } __ss + __sc }");
            } else {
                // PMAT-595: integer `sum` honors C-PY-INT-ARITH via a checked
                // fold seeded with `start` (matches the Rust backend).
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
            // PMAT-689: short-circuiting (genexpr) any/all over a Map fuses the
            // predicate into the any/all closure (see the rust backend).
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
        // PMAT-502k: `seq * n` → `(seq).repeat(((n).max(0)) as usize)`.
        Expr::Repeat { seq, n, of_str } => {
            if *of_str {
                out.push('(');
                emit_expr(out, seq, mode)?;
                out.push_str(").repeat(((");
                emit_expr(out, n, mode)?;
                out.push_str(").max(0)) as usize)");
            } else {
                // PMAT-569: list repeat clones elements (see Rust twin).
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
            // PMAT-502bf: string parse, matching the Rust backend.
            if *from_str && *to_float {
                // PMAT-611: float(s) accepts PEP 515 underscores between digits
                // (matches the Rust backend). Bind a reference so a temporary
                // operand survives the block via lifetime extension (E0716).
                out.push_str("{ let __pf = &(");
                emit_expr(out, value, mode)?;
                // PMAT-1089: both panics quote the ORIGINAL (untrimmed)
                // argument like CPython, and the parse failure formats via
                // `unwrap_or_else` (mirror rust).
                out.push_str("); let __ps = __pf.trim(); let __pe = __ps.as_bytes(); if !__ps.bytes().enumerate().all(|(__k, __c)| __c != b'_' || (__k > 0 && __pe[__k - 1].is_ascii_digit() && __k + 1 < __pe.len() && __pe[__k + 1].is_ascii_digit())) { panic!(\"xpile: ValueError: could not convert string to float: {}\", ");
                out.push_str(&py_str_repr_block("__pf"));
                out.push_str("); } __ps.replace('_', \"\").parse::<f64>().unwrap_or_else(|_| panic!(\"xpile: ValueError: could not convert string to float: {}\", ");
                out.push_str(&py_str_repr_block("__pf"));
                out.push_str(")) }");
            } else if *from_str {
                // PMAT-610: int(s) accepts PEP 515 underscores between digits
                // (matches the Rust backend). Bind a reference so a temporary
                // operand survives the block via lifetime extension (E0716).
                out.push_str("{ let __pf = &(");
                emit_expr(out, value, mode)?;
                // PMAT-1089: CPython message shape `invalid literal for int()
                // with base 10: '<orig>'` on both panics (mirror rust).
                // PMAT-1097: three-way parse-failure classification (mirror
                // rust) — all-ASCII-digit body that fails parse is i64 overflow
                // (CPython bigint) → honest range message; all-numeric body
                // with non-ASCII chars is CPython's Unicode-decimal acceptance
                // → honest digit-class refusal; only the rest is a genuine
                // CPython ValueError with the exact invalid-literal message.
                out.push_str("); let __ps = __pf.trim(); let __pb = __ps.strip_prefix('-').or_else(|| __ps.strip_prefix('+')).unwrap_or(__ps); if __pb.starts_with('_') || __pb.ends_with('_') || __pb.contains(\"__\") { panic!(\"xpile: ValueError: invalid literal for int() with base 10: {}\", ");
                out.push_str(&py_str_repr_block("__pf"));
                out.push_str("); } let __pc = __ps.replace('_', \"\"); let __pd = __pb.replace('_', \"\"); if !__pd.is_empty() && __pd.chars().all(|__c| __c.is_ascii_digit()) { __pc.parse::<i64>().unwrap_or_else(|_| panic!(\"xpile: int() out of i64 range; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\")) } else if !__pd.is_empty() && __pd.chars().all(|__c| __c.is_numeric()) { panic!(\"xpile: int() with non-ASCII digits: CPython accepts Unicode decimal digits; not yet implemented\") } else { __pc.parse::<i64>().unwrap_or_else(|_| panic!(\"xpile: ValueError: invalid literal for int() with base 10: {}\", ");
                out.push_str(&py_str_repr_block("__pf"));
                out.push_str(")) } }");
            } else if !*to_float && *from_float {
                // PMAT-586: `int(float_x)` guards a non-finite source (see Rust twin).
                out.push_str("{ let __ic = ");
                emit_expr(out, value, mode)?;
                // PMAT-793 (HUNT-V18 EXC-002): tag the non-finite panics with the
                // exact Python exception (nan → ValueError, ±inf → OverflowError)
                // so the allowlist `except` discriminates them (mirror Rust backend).
                out.push_str("; if __ic.is_nan() { panic!(\"xpile: ValueError: cannot convert float NaN to integer\"); } if __ic.is_infinite() { panic!(\"xpile: OverflowError: cannot convert float infinity to integer\"); } if __ic < (i64::MIN as f64) || __ic >= (i64::MAX as f64) { panic!(\"xpile: int() out of i64 range; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); } __ic as i64 }");
            } else {
                out.push_str("((");
                emit_expr(out, value, mode)?;
                out.push_str(if *to_float { ") as f64)" } else { ") as i64)" });
            }
        }
        // PMAT-502ad/af: `str(x)` → `format!("{}", x)` (int) or a
        // Python-matching format block (float).
        Expr::ToStr { value, of_float } => {
            if *of_float {
                // PMAT-583/842: CPython float repr — shared with the dataclass
                // Display path via `py_float_repr_block` (see the Rust twin).
                let mut v = String::new();
                emit_expr(&mut v, value, mode)?;
                out.push_str(&py_float_repr_block(&v));
            } else {
                out.push_str("format!(\"{}\", ");
                emit_expr(out, value, mode)?;
                out.push(')');
            }
        }
        // PMAT-582/778/842: `repr(str)` — shared with the dataclass Display path
        // via `py_str_repr_block` (see the Rust twin).
        Expr::ReprStr { value } => {
            let mut v = String::new();
            emit_expr(&mut v, value, mode)?;
            out.push_str(&py_str_repr_block(&v));
        }
        // PMAT-502ak: `round(x)` (float) → `((x).round_ties_even() as i64)`.
        Expr::RoundToInt { value } => {
            // PMAT-664: guard inf/nan + i64 range (see the rust backend).
            out.push_str("{ let __rti = (");
            emit_expr(out, value, mode)?;
            out.push_str(").round_ties_even(); if !__rti.is_finite() { panic!(\"xpile: round() of a non-finite float (Python OverflowError/ValueError)\"); } if __rti < (i64::MIN as f64) || __rti >= (i64::MAX as f64) { panic!(\"xpile: round() out of i64 range; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented\"); } __rti as i64 }");
        }
        // PMAT-502al: `round(x, n)` (float) → Python's decimal rounding
        // (format-to-n-decimals for n >= 0, scale for n < 0). PMAT-870
        // (HUNT-V31 #9): guard the n <= -309 overflow (`10f64.powi(-n)` → +inf →
        // `0.0 * inf` = NaN); Python rounds to 0, so return a sign-preserving
        // zero (mirror the Rust backend).
        Expr::RoundToDigits { value, ndigits } => {
            out.push_str("{ let __rx = ");
            emit_expr(out, value, mode)?;
            out.push_str("; let __rn = ");
            emit_expr(out, ndigits, mode)?;
            out.push_str("; if __rn >= 0 { format!(\"{:.1$}\", __rx, __rn as usize).parse::<f64>().unwrap() } else { let __rp = 10f64.powi((-__rn) as i32); if __rp.is_infinite() { __rx * 0.0 } else { (__rx / __rp).round_ties_even() * __rp } } }");
        }
        // PMAT-612: `round(int, n)` → int (banker's rounding for n < 0, identity
        // for n >= 0; i128 arithmetic, fails loud out of i64 range). Mirrors the
        // Rust backend.
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
        // PMAT-502e/h/aa: 1-arg `min(xs)`/`max(xs)`; `key=lambda` →
        // `min_by_key`/`max_by_key`.
        Expr::ListMinMax {
            list,
            is_max,
            of_float,
            of_struct_cmp,
            key,
            default,
        } => {
            // PMAT-502dh: an optional `default` → `.unwrap_or(<default>)` on
            // the empty case; the float branch uses `.reduce(..)` with default.
            emit_expr(out, list, mode)?;
            match key {
                // PMAT-653: float-returning key → max_by/min_by + partial_cmp
                // (f64 isn't Ord). See the rust backend.
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
                    // PMAT-568: Python max(key=) returns the FIRST maximal element
                    // (Rust max_by_key returns the last) — reverse first. min ok.
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
                // PMAT-889 (HUNT-V33 #4): struct element w/ custom __lt__
                // (PartialOrd, not Ord, not Copy) → `.cloned().max_by(partial_cmp)`
                // (`max` reverses first for first-wins ties). See the rust backend.
                None if *of_struct_cmp => {
                    if *is_max {
                        out.push_str(".iter().cloned().rev().max_by(|__a, __b| __a.partial_cmp(__b).unwrap_or(std::cmp::Ordering::Equal))");
                    } else {
                        out.push_str(".iter().cloned().min_by(|__a, __b| __a.partial_cmp(__b).unwrap_or(std::cmp::Ordering::Equal))");
                    }
                }
                None => match *of_float {
                    // PMAT-502er: `.cloned()` (not `.copied()`) so non-Copy
                    // `String` min/max works too; i64/bool are `Clone`.
                    false => out.push_str(if *is_max {
                        ".iter().cloned().max()"
                    } else {
                        ".iter().cloned().min()"
                    }),
                    // PMAT-608: float min/max = first-arg-wins reduce (matches
                    // the Rust backend); empty → Option (ValueError, not ±∞).
                    true => {
                        let cmp = if *is_max { ">" } else { "<" };
                        write!(
                            out,
                            ".iter().copied().reduce(|__a, __b| if __b {cmp} __a {{ __b }} else {{ __a }})"
                        )?;
                    }
                },
            }
            match default {
                Some(d) => {
                    out.push_str(".unwrap_or(");
                    emit_expr(out, d, mode)?;
                    out.push(')');
                }
                // PMAT-774 (HUNT-V16 CG-5): tag the empty-sequence panic with the
                // canonical `xpile: ValueError: <fn>() arg is an empty sequence`
                // (mirror of the Rust backend) so a typed `except ValueError`
                // catches it; the int branch was a bare `.unwrap()`.
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
                // PMAT-853: compare by place (`**__e == arg`) so a non-Copy element
                // type (String, …) works too (mirror the Rust backend).
                ListQueryOp::Count => {
                    out.push_str(".iter().filter(|__e| **__e == ");
                    emit_expr(out, arg, mode)?;
                    out.push_str(").count() as i64");
                }
                ListQueryOp::Index => {
                    // `position` yields `&T` (one ref) — single deref.
                    out.push_str(".iter().position(|__e| *__e == ");
                    emit_expr(out, arg, mode)?;
                    out.push_str(").map(|__i| __i as i64).expect(\"xpile: ValueError: list.index(x): x not in list\")");
                }
            }
        }
        // PMAT-502as: `xs.pop()` / `xs.pop(i)`, matching the Rust backend.
        // PMAT-570: a negative-resolved index (`len-k`) references the receiver —
        // bind it before remove() (E0502). Positive indices keep the inline form.
        Expr::ListPop { list, index } => match index {
            None => {
                // PMAT-715: `xs[i].pop()` → l-value pop (mirror rust); the read
                // path clones the inner container, popping a throwaway clone.
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
                    // PMAT-797 (HUNT-V19 ND-01): `d[k].pop()` mutates the stored
                    // list in place via get_mut (the dict read clones); mirror of
                    // the Rust backend.
                    // PMAT-1089: bind the key first so the miss panic carries
                    // the CPython-shaped `repr(k)` payload.
                    out.push_str("{ let __k = &(");
                    emit_expr(out, key, mode)?;
                    out.push_str("); (");
                    emit_expr(out, dict, mode)?;
                    out.push_str(").get_mut(__k).unwrap_or_else(|| ");
                    out.push_str(&key_error_panic());
                    out.push_str(").pop().expect(\"xpile: IndexError: pop from empty list\") }");
                } else if let Some((base, idx)) = lvalue_base {
                    out.push_str("{ let __pi = (");
                    emit_expr(out, idx, mode)?;
                    write!(
                        out,
                        ") as i64; let __pi = if __pi < 0 {{ {base}.len() as i64 + __pi }} else {{ __pi }}; {base}[__pi as usize].pop().expect(\"xpile: IndexError: pop from empty list\") }}"
                    )?;
                } else {
                    // PMAT-747 (HUNT-V14 #2): tag the empty-list-pop panic.
                    out.push('(');
                    emit_expr(out, list, mode)?;
                    out.push_str(").pop().expect(\"xpile: IndexError: pop from empty list\")");
                }
            }
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
        // PMAT-502au: `d.pop(k)` / `d.pop(k, def)`, matching the Rust backend.
        Expr::DictPop { dict, key, default } => {
            match default {
                // PMAT-747 (HUNT-V14 #2): tag the absent-key dict-pop panic.
                // PMAT-1089: bind the key first so the panic carries the
                // CPython-shaped `repr(k)` payload (mirror rust).
                None => {
                    out.push_str("{ let __k = &(");
                    emit_expr(out, key, mode)?;
                    out.push_str("); (");
                    emit_expr(out, dict, mode)?;
                    out.push_str(").shift_remove(__k).unwrap_or_else(|| ");
                    out.push_str(&key_error_panic());
                    out.push_str(") }");
                }
                Some(d) => {
                    out.push('(');
                    emit_expr(out, dict, mode)?;
                    out.push_str(").shift_remove(&(");
                    emit_expr(out, key, mode)?;
                    out.push_str(")).unwrap_or(");
                    emit_expr(out, d, mode)?;
                    out.push(')');
                }
            }
        }
        // PMAT-502ax / PMAT-843: `d.setdefault(k, default)` — bind the default
        // before `.entry()` so a dict-reading default doesn't E0502 (mirror Rust).
        Expr::DictSetDefault { dict, key, default } => {
            out.push_str("{ let __sd_def = ");
            emit_expr(out, default, mode)?;
            out.push_str("; (");
            emit_expr(out, dict, mode)?;
            out.push_str(").entry((");
            emit_expr(out, key, mode)?;
            out.push_str(").clone()).or_insert(__sd_def).clone() }");
        }
        // PMAT-502c/f/z: clone+sort block; `reverse=True` appends
        // `__xv.reverse();`; `key=lambda p: e` → `sort_by_key`.
        Expr::Sorted {
            list,
            reverse,
            key,
            of_float,
        } => {
            out.push_str("{ let mut __xv = ");
            emit_expr(out, list, mode)?;
            out.push_str(".clone(); __xv.");
            // PMAT-568: reverse=True + key must be STABLE descending (see Rust twin).
            // PMAT-578: keyless float sort uses `sort_by(partial_cmp)` (no `Ord`).
            // PMAT-616: NaN-safe — fall back to `Equal` (Python doesn't raise on NaN).
            match (key, *reverse) {
                (None, false) if *of_float => {
                    out.push_str("sort_by(|__a, __b| __a.partial_cmp(__b).unwrap_or(std::cmp::Ordering::Equal));");
                }
                (None, true) if *of_float => {
                    out.push_str("sort_by(|__a, __b| __b.partial_cmp(__a).unwrap_or(std::cmp::Ordering::Equal));");
                }
                (None, false) => out.push_str("sort();"),
                (None, true) => out.push_str("sort(); __xv.reverse();"),
                // PMAT-603: float key → partial_cmp (no Ord); matches Rust twin.
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
                    if *of_float {
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
        // PMAT-549: `math.gcd(a, b)` → inline Euclidean algorithm (abs values).
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
        // PMAT-571: `pow(base, exp, mod)` → modular exponentiation (see Rust twin).
        Expr::PowMod { base, exp, modulus } => {
            out.push_str("{ let __pmm = (");
            emit_expr(out, modulus, mode)?;
            out.push_str("); if __pmm == 0 { panic!(\"xpile: ValueError: pow() 3rd argument cannot be 0\"); } let __pme = (");
            emit_expr(out, exp, mode)?;
            out.push_str("); if __pme < 0 { panic!(\"xpile: ValueError: pow() 2nd argument cannot be negative when 3rd argument specified\"); } let __pmb0 = (");
            emit_expr(out, base, mode)?;
            // PMAT-619: modexp on the magnitude |m| (i128), sign-correct at the
            // end — matches the Rust backend (a negative modulus, esp. with a
            // negative base, previously gave the wrong sign/value).
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
                // PMAT-523: negative-step range (Ruchy → Rust).
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
        // PMAT-520: `list(<set>)` / `sorted(<set>)` → unique elements as a Vec.
        Expr::SetToList { set } => {
            emit_expr(out, set, mode)?;
            out.push_str(".iter().cloned().collect::<Vec<_>>()");
        }
        // PMAT-502dk: `dict(pairs)` → a HashMap from the list of 2-tuples.
        Expr::DictFromPairs { pairs } => {
            emit_expr(out, pairs, mode)?;
            out.push_str(".iter().cloned().collect::<indexmap::IndexMap<_, _>>()");
        }
        // PMAT-502dw/dx: `{k: v, **d, …}` → chain each fragment's iterator into
        // a fresh HashMap (a later entry wins, matching Python).
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
        // PMAT-502ab: `filter(pred, xs)` → `.iter().cloned().filter(...).collect()`.
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
        // PMAT-502ac: `map(f, xs)` → `.iter().cloned().map(...).collect()`.
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
            // PMAT-684: `start` offsets the index (see the rust backend).
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
        // PMAT-462 (v0.2.0 Track 1.C): Ruchy → Rust HashMap-init block.
        // PMAT-466: empty literal → bare `HashMap::new()` (see the Rust
        // backend's twin arm — avoids clippy `unused_mut`).
        Expr::DictLit(pairs) => {
            if pairs.is_empty() {
                out.push_str("indexmap::IndexMap::new()");
            } else {
                // PMAT-720 (HUNT-V8 V8-EXTRA): accumulator named `__xpile_map`,
                // not `m` — a user variable `m` would otherwise be shadowed and the
                // bare-ident key/value would reference the HashMap (mirrors rust).
                out.push_str("{ let mut __xpile_map = indexmap::IndexMap::new(); ");
                for (k, v) in pairs {
                    // PMAT-699: clone bare-ident keys/values to avoid the
                    // move-then-reuse E0382 (mirrors the rust backend).
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
        // PMAT-457 (v0.2.0 Track 1.B): Ruchy → Rust →
        // `xs[i as usize].clone()`, matching the Rust backend.
        // PMAT-639: runtime-negative list index wraps like Python (mirrors the
        // Rust backend); a non-negative literal index keeps the fast path.
        Expr::Index { collection, index } => {
            let nonneg_literal = matches!(index.as_ref(), Expr::LitInt(n) if *n >= 0);
            if nonneg_literal {
                // PMAT-764 (HUNT-V16 #4): tag the literal-index OOB panic with
                // `xpile: IndexError:` (mirror of the Rust backend) so a typed
                // `except` discriminates it instead of swallowing the native panic.
                out.push_str("{ let __lc = &(");
                emit_expr(out, collection, mode)?;
                out.push_str("); let __li = (");
                emit_expr(out, index, mode)?;
                out.push_str(") as usize; if __li >= __lc.len() { panic!(\"xpile: IndexError: list index out of range\"); } __lc[__li].clone() }");
            } else {
                // PMAT-744 (HUNT-V13 exc-flow-01/02): tag the out-of-bounds panic
                // as `xpile: IndexError:` (mirrors Rust) so typed-`except`
                // discrimination re-raises/catches it correctly instead of an
                // untagged native bounds panic being silently swallowed.
                out.push_str("{ let __lc = &(");
                emit_expr(out, collection, mode)?;
                out.push_str("); let __li: i64 = (");
                emit_expr(out, index, mode)?;
                out.push_str(") as i64; let __lidx = if __li < 0 { __lc.len() as i64 + __li } else { __li }; if __lidx < 0 || __lidx as usize >= __lc.len() { panic!(\"xpile: IndexError: list index out of range\"); } __lc[__lidx as usize].clone() }");
            }
        }
        // PMAT-466 (v0.2.0 Track 1.C): dict ops → Rust, matching the
        // Rust backend exactly (Ruchy compiles to Rust).
        Expr::DictGet { dict, key } => {
            // PMAT-747 (HUNT-V14 #2): tag an absent-key dict-index miss with
            // `xpile: KeyError:` so typed-`except` discrimination works (mirrors
            // the rust backend); HashMap's native `Index` panic was untagged.
            // PMAT-1089: the key binds first (`__k`, a reference — no move) so
            // the miss panic carries the CPython-shaped `repr(k)` payload.
            out.push_str("{ let __k = &(");
            emit_expr(out, key, mode)?;
            out.push_str("); (");
            emit_expr(out, dict, mode)?;
            out.push_str(").get(__k).cloned().unwrap_or_else(|| ");
            out.push_str(&key_error_panic());
            out.push_str(") }");
        }
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
        // PMAT-500/501b: Ruchy → Rust set literal (empty → bare new()).
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
        // PMAT-502g: set algebra → fresh HashSet via `.cloned().collect()`.
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
        // PMAT-502ep: set predicate — matching the Rust backend.
        Expr::SetPred { lhs, op, rhs } => {
            // PMAT-652: bind operands by reference (see the rust backend) so a
            // reused / self-compared set operand isn't moved (E0382).
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
        // PMAT-502eq: `.copy()` → `(<inner>).clone()`, matching the Rust backend.
        Expr::Clone(inner) => {
            out.push('(');
            emit_expr(out, inner, mode)?;
            out.push_str(").clone()");
        }
        // PMAT-502ew: `Option` value — `None` / `Some(<e>)`, matching Rust.
        Expr::OptionExpr(inner) => match inner {
            None => out.push_str("None"),
            Some(e) => {
                out.push_str("Some(");
                emit_expr(out, e, mode)?;
                out.push(')');
            }
        },
        // PMAT-721 (HUNT-V9 V9-18): Optional truthiness →
        // `(<value>)[.as_ref()].is_some_and(|__v| <body>)` (mirrors the Rust backend).
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
        // `(value).filter(|<param>| <body>).unwrap_or_else(|| <default>)` (mirrors
        // the Rust backend).
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
        // PMAT-502ex: `x is None`/`is not None` → `.is_none()`/`.is_some()`.
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
        // PMAT-506b: struct construction `Name { f0: v0, … }` (Ruchy → Rust).
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
        // PMAT-503b: try/except → catch_unwind match (Ruchy compiles to Rust).
        Expr::TryCatch {
            body,
            handler,
            except_types,
            bound_name,
        } => {
            // PMAT-789 (HUNT-V18 EXC-001): a typed `except T:` / tuple `except (A,
            // B):` catches ONLY a panic whose payload names one of the LISTED types
            // (`xpile: <T>: …`) and re-raises everything else — an ALLOWLIST
            // (inversion of the prior blocklist, which swallowed RuntimeError / any
            // non-cataloged or untagged panic). Mirrors the Rust backend. A
            // catch-all (empty set) keeps `Err(_)`.
            // PMAT-817 (HUNT-V20 EXC-4): `except E as e:` binds the prefix-stripped
            // exception message to a `String` local `e` (mirrors the Rust backend).
            let bind = |out: &mut String, name: &str| {
                write!(
                    out,
                    "let {name} = __xpile_m.strip_prefix(\"xpile: \").and_then(|__s| __s.splitn(2, \": \").nth(1)).unwrap_or(__xpile_m).to_string(); "
                )
            };
            out.push_str("match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| ");
            emit_expr(out, body, mode)?;
            out.push_str(")) { Ok(__xpile_try) => __xpile_try, ");
            // PMAT-1105 (c): the catch-all is GATED — Python exceptions only;
            // a capability/honesty refusal re-raises (mirrors the Rust
            // backend; the old unconditional `Err(_)` arm swallowed them).
            if except_types.is_empty() {
                write!(out, "Err(__xpile_e) => {{ let __xpile_m: &str = __xpile_e.downcast_ref::<String>().map(|__s| __s.as_str()).or_else(|| __xpile_e.downcast_ref::<&str>().copied()).unwrap_or(\"\"); if {IS_PY_EXC_PRED} {{ ")?;
                if let Some(name) = bound_name {
                    bind(out, name)?;
                }
                emit_expr(out, handler, mode)?;
                out.push_str(" } else { ::std::panic::resume_unwind(__xpile_e) } }");
            } else {
                out.push_str("Err(__xpile_e) => { let __xpile_m: &str = __xpile_e.downcast_ref::<String>().map(|__s| __s.as_str()).or_else(|| __xpile_e.downcast_ref::<&str>().copied()).unwrap_or(\"\"); if ");
                for (i, k) in except_types.iter().enumerate() {
                    if i > 0 {
                        out.push_str(" || ");
                    }
                    write_exc_tag_pred(out, k)?;
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
        // PMAT-459 (v0.2.0 Track 1.B): Ruchy → Rust → `.len() as i64`.
        // PMAT-1074: `open(path).read()` → inline std::fs::read_to_string.
        // PMAT-1081: borrowed path (no move on a variable path) + universal-
        // newline normalization (CRLF then lone CR — CPython text mode) +
        // PermissionDenied → PermissionError. Mirrors the rust codegen.
        Expr::FileReadLines(path) => {
            out.push_str("::std::fs::read_to_string(&(");
            emit_expr(out, path, mode)?;
            out.push_str(")).unwrap_or_else(|__e| if __e.kind() == ::std::io::ErrorKind::NotFound { panic!(\"xpile: FileNotFoundError: {}\", __e) } else if __e.kind() == ::std::io::ErrorKind::PermissionDenied { panic!(\"xpile: PermissionError: {}\", __e) } else { panic!(\"xpile: OSError: {}\", __e) }).replace(\"\\r\\n\", \"\\n\").replace('\\r', \"\\n\").split_inclusive('\\n').map(|__l| __l.to_string()).collect::<Vec<String>>()");
        }
        Expr::FileReadAll(path) => {
            out.push_str("::std::fs::read_to_string(&(");
            emit_expr(out, path, mode)?;
            out.push_str(")).unwrap_or_else(|__e| if __e.kind() == ::std::io::ErrorKind::NotFound { panic!(\"xpile: FileNotFoundError: {}\", __e) } else if __e.kind() == ::std::io::ErrorKind::PermissionDenied { panic!(\"xpile: PermissionError: {}\", __e) } else { panic!(\"xpile: OSError: {}\", __e) }).replace(\"\\r\\n\", \"\\n\").replace('\\r', \"\\n\")");
        }
        Expr::Len(inner) => {
            // PMAT-761 (HUNT-V16 CFD-3): parenthesize the cast so `len(x) < N`
            // doesn't make rustc read `i64 <` as a turbofish (parse error).
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
        // PMAT-449 (v0.2.0 Track 1.A): Python `str` literal → Ruchy
        // owned `String::from("...")`. Same escape semantics as the
        // Rust backend.
        Expr::LitStr(s) => {
            write!(out, "String::from(\"{}\")", escape_ruchy_str(s))?;
        }
        // PMAT-042: `QuotedString` carries explicit shell quoting and
        // stays bashrs-only.
        Expr::QuotedString { .. } => {
            return Err(RuchyCodegenError::Unsupported(
                "Ruchy backend does not lower Expr::QuotedString — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs quoted shell strings; \
                 use `--target shell`"
                    .into(),
            ));
        }
        // PMAT-045: see rust-codegen's matching arm.
        Expr::ShellVar(name) => {
            return Err(RuchyCodegenError::Unsupported(format!(
                "Ruchy backend does not lower Expr::ShellVar (${name}) — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell variable refs; \
                 use `--target shell`"
            )));
        }
        // PMAT-047: see rust-codegen.
        Expr::CommandSubstitution(_) => {
            return Err(RuchyCodegenError::Unsupported(
                "Ruchy backend does not lower Expr::CommandSubstitution — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell substitution; \
                 use `--target shell`"
                    .into(),
            ));
        }
        // PMAT-055: see rust-codegen.
        Expr::ShellSpecial(name) => {
            return Err(RuchyCodegenError::Unsupported(format!(
                "Ruchy backend does not lower Expr::ShellSpecial (${name}) — \
                 contract C-BASHRS-POSIX-IDEMPOTENCE governs shell special params; \
                 use `--target shell`"
            )));
        }
    }
    Ok(())
}

fn emit_unop(
    out: &mut String,
    op: UnOp,
    operand: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    match op {
        UnOp::Neg => {
            if mode {
                // BigInt::neg is total — no overflow.
                write!(out, "(-")?;
                emit_expr(out, operand, mode)?;
                write!(out, ")")?;
            } else {
                // Python: `-x` on int never overflows mathematically.
                // Rust i64::MIN.checked_neg() == None — use checked_neg
                // + panic pointing at C-PY-INT-ARITH slow path.
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
        // PMAT-502fb: Python `~x` == Rust `!x` on a signed integer.
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
) -> Result<(), RuchyCodegenError> {
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

/// Ruchy uses Rust-like `if cond { then } else { else_ }` as an expression.
/// Flattens nested `else if` for readability (same pattern as the Rust backend).
fn emit_if_expr(
    out: &mut String,
    cond: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
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
        } => emit_if_expr(out, c2, t2, e2, mode),
        _ => {
            write!(out, "{{ ")?;
            emit_expr(out, else_expr, mode)?;
            write!(out, " }}")?;
            Ok(())
        }
    }
}

/// Arithmetic emits two shapes per the C-PY-INT-ARITH contract:
///
/// * i64 fast path: `.checked_*().expect("...")` with the slow-path
///   panic message (no overflow → no panic).
/// * BigInt slow path (mode=true): plain infix on BigInt operands
///   (BigInt overloads `+ - * <= ...`); FloorDiv / Mod use
///   `xpile_bigint::div_floor / mod_floor`; bitwise / shift / pow
///   deferred (same scope as the Rust backend).
///
/// Mirrors the Rust backend's emission shape — Ruchy compiles to Rust
/// so they share semantics. PMAT-025.
fn emit_binop(
    out: &mut String,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    match op {
        BinOp::Add if mode => emit_infix(out, lhs, " + ", rhs, mode),
        BinOp::Sub if mode => emit_infix(out, lhs, " - ", rhs, mode),
        BinOp::Mul if mode => emit_infix(out, lhs, " * ", rhs, mode),
        BinOp::FloorDiv if mode => emit_bigint_floor_call(out, "div_floor", lhs, rhs, mode),
        BinOp::Mod if mode => emit_bigint_floor_call(out, "mod_floor", lhs, rhs, mode),
        // PMAT-026 / PMAT-013-FOLLOWUP — mirror of the Rust backend.
        // See `xpile-rust-codegen/src/lib.rs` for the design rationale.
        BinOp::BitAnd if mode => emit_infix(out, lhs, " & ", rhs, mode),
        BinOp::BitOr if mode => emit_infix(out, lhs, " | ", rhs, mode),
        BinOp::BitXor if mode => emit_infix(out, lhs, " ^ ", rhs, mode),
        BinOp::Shl if mode => emit_bigint_floor_call(out, "shl", lhs, rhs, mode),
        BinOp::Shr if mode => emit_bigint_floor_call(out, "shr", lhs, rhs, mode),
        BinOp::Pow if mode => emit_bigint_floor_call(out, "pow", lhs, rhs, mode),
        BinOp::Add => emit_checked(out, lhs, "checked_add", rhs, "addition", mode),
        BinOp::Sub => emit_checked(out, lhs, "checked_sub", rhs, "subtraction", mode),
        BinOp::Mul => emit_checked(out, lhs, "checked_mul", rhs, "multiplication", mode),
        // PMAT-538: euclidean div/rem only match Python `//`/`%` for a positive
        // divisor; emit the truncating quotient/remainder with a floor
        // correction (mirrors the Rust backend).
        BinOp::FloorDiv => emit_floor_div(out, lhs, rhs, mode),
        BinOp::Mod => emit_floor_mod(out, lhs, rhs, mode),
        // PMAT-618: `d.get(k) == v` / `!= v` — wrap the bare-value side in
        // `Some(...)` so a no-default `d.get` (`Option<T>`) compares as
        // `Option<T> == Some(v)`, matching Python (`None == v` is False).
        // Matches the Rust backend.
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

/// BigInt-mode floor-div / mod via the helpers in xpile-bigint
/// (num-bigint requires `Integer` trait + reference operands).
/// PMAT-025; mirrors Rust backend.
fn emit_bigint_floor_call(
    out: &mut String,
    method: &str,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    write!(out, "xpile_bigint::{method}(&")?;
    emit_expr(out, lhs, mode)?;
    write!(out, ", &")?;
    emit_expr(out, rhs, mode)?;
    write!(out, ")")?;
    Ok(())
}

fn emit_checked_pow(
    out: &mut String,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
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

fn emit_checked_shift(
    out: &mut String,
    lhs: &Expr,
    method: &str,
    rhs: &Expr,
    op_name: &str,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    // PMAT-575: see the Rust backend's twin — `checked_shl` only guards the
    // shift amount, not value overflow (`1 << 63` wraps to i64::MIN silently),
    // falsifying C-PY-INT-ARITH. Emit a reversibility check for left-shift.
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
    // PMAT-577: see the Rust backend's twin — Python `x >> n` saturates to the
    // sign fill for n >= 64 (0 / -1), but `checked_shr` panics; clamp to 63.
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

fn emit_checked(
    out: &mut String,
    lhs: &Expr,
    method: &str,
    rhs: &Expr,
    op_name: &str,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
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

/// PMAT-538: Python floor-division `a // b` for i64 — truncating quotient with a
/// floor correction (Python floors toward −∞; `div_euclid` diverges for a
/// negative divisor). Mirrors the Rust backend.
fn emit_floor_div(
    out: &mut String,
    lhs: &Expr,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    let panic_msg = "xpile: i64 floor-div overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented";
    write!(out, "{{ let __fa = ")?;
    emit_expr(out, lhs, mode)?;
    write!(out, "; let __fb = ")?;
    emit_expr(out, rhs, mode)?;
    // PMAT-728: zero-divisor → ZeroDivisionError before checked_div (mirrors rust).
    write!(
        out,
        "; if __fb == 0 {{ panic!(\"xpile: ZeroDivisionError: integer division or modulo by zero\"); }} \
         let __q = __fa.checked_div(__fb).expect(\"{panic_msg}\"); \
         let __r = __fa.checked_rem(__fb).expect(\"{panic_msg}\"); \
         if __r != 0 && (__r < 0) != (__fb < 0) {{ __q - 1 }} else {{ __q }} }}"
    )?;
    Ok(())
}

/// PMAT-538: Python modulo `a % b` for i64 — truncating remainder with a floor
/// correction (Python's `%` takes the sign of the divisor). Mirrors the Rust
/// backend.
/// PMAT-740 (HUNT-V12 V12-24): widen an i64-typed `*` tree to i128 (mirrors the
/// Rust backend) so `(a*b) % m` doesn't overflow the intermediate product.
fn emit_mul_tree_as_i128(out: &mut String, e: &Expr, mode: bool) -> Result<(), RuchyCodegenError> {
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
) -> Result<(), RuchyCodegenError> {
    let panic_msg = "xpile: i64 modulo overflow; bigint promotion (contract C-PY-INT-ARITH slow path) not yet implemented";
    // PMAT-740 (HUNT-V12 V12-24): `(a*b) % m` — widen the product + floor-mod to
    // i128 so the intermediate doesn't overflow (matches the Rust backend).
    if !mode && matches!(lhs, Expr::BinOp { op: BinOp::Mul, .. }) {
        write!(out, "{{ let __mm: i128 = ")?;
        emit_mul_tree_as_i128(out, lhs, mode)?;
        write!(out, "; let __md: i128 = (")?;
        emit_expr(out, rhs, mode)?;
        write!(
            out,
            ") as i128; if __md == 0 {{ panic!(\"xpile: ZeroDivisionError: integer division or modulo by zero\"); }} \
             let __r = __mm % __md; \
             (if __r != 0 && (__r < 0) != (__md < 0) {{ __r + __md }} else {{ __r }}) as i64 }}"
        )?;
        return Ok(());
    }
    write!(out, "{{ let __fa = ")?;
    emit_expr(out, lhs, mode)?;
    write!(out, "; let __fb = ")?;
    emit_expr(out, rhs, mode)?;
    // PMAT-728: zero-divisor → ZeroDivisionError before checked_rem (mirrors rust).
    // PMAT-1097: message pinned to the CPython 3.10 oracle ground truth —
    // "integer division or modulo by zero" for both `//` and `%` (mirrors rust).
    write!(
        out,
        "; if __fb == 0 {{ panic!(\"xpile: ZeroDivisionError: integer division or modulo by zero\"); }} \
         let __r = __fa.checked_rem(__fb).expect(\"{panic_msg}\"); \
         if __r != 0 && (__r < 0) != (__fb < 0) {{ __r + __fb }} else {{ __r }} }}"
    )?;
    Ok(())
}

fn emit_infix(
    out: &mut String,
    lhs: &Expr,
    op: &str,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    write!(out, "(")?;
    emit_expr(out, lhs, mode)?;
    out.push_str(op);
    emit_expr(out, rhs, mode)?;
    write!(out, ")")?;
    Ok(())
}

/// PMAT-745 (HUNT-V13 intfloat-cmp-precision): emit an EXACT `int OP float`
/// comparison (mirror of the rust-codegen helper). Python never rounds the int
/// operand to `f64`; the block compares `__cn as f64` against `__cf` for strict
/// ordering and breaks the equality tie in `i128` (which exactly holds every
/// integral `f64` an `i64` can reach, up to 2^63). NaN falls through every arm
/// (Python: `n != nan` is `True`, the rest `False`). The frontend normalises
/// `op` to the int-on-left form, so only the six comparison operators appear.
fn emit_mixed_int_float_cmp(
    out: &mut String,
    int: &Expr,
    float: &Expr,
    op: BinOp,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    let body = match op {
        BinOp::Eq => "__cnf == __cf && (__cn as i128) == (__cf as i128)",
        BinOp::NotEq => "__cnf != __cf || (__cn as i128) != (__cf as i128)",
        BinOp::Lt => "__cnf < __cf || (__cnf == __cf && (__cn as i128) < (__cf as i128))",
        BinOp::LtEq => "__cnf < __cf || (__cnf == __cf && (__cn as i128) <= (__cf as i128))",
        BinOp::Gt => "__cnf > __cf || (__cnf == __cf && (__cn as i128) > (__cf as i128))",
        BinOp::GtEq => "__cnf > __cf || (__cnf == __cf && (__cn as i128) >= (__cf as i128))",
        other => {
            return Err(RuchyCodegenError::Unsupported(format!(
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

/// PMAT-618: is this a no-default `d.get(k)` (an `Option<T>`)?
fn is_dict_get_opt(e: &Expr) -> bool {
    matches!(e, Expr::DictGetOpt { .. })
}

/// PMAT-618: `==`/`!=` where exactly one operand is a no-default `d.get(k)`;
/// the bare-value side is wrapped in `Some(...)` (matches the Rust backend).
fn emit_opt_eq(
    out: &mut String,
    lhs: &Expr,
    op: &str,
    rhs: &Expr,
    mode: bool,
) -> Result<(), RuchyCodegenError> {
    write!(out, "(")?;
    emit_opt_eq_operand(out, lhs, mode)?;
    out.push_str(op);
    emit_opt_eq_operand(out, rhs, mode)?;
    write!(out, ")")?;
    Ok(())
}

fn emit_opt_eq_operand(out: &mut String, e: &Expr, mode: bool) -> Result<(), RuchyCodegenError> {
    if is_dict_get_opt(e) {
        emit_expr(out, e, mode)
    } else {
        out.push_str("Some(");
        emit_expr(out, e, mode)?;
        out.push(')');
        Ok(())
    }
}

pub struct RuchyBackend;

impl Backend for RuchyBackend {
    fn name(&self) -> &'static str {
        "ruchy"
    }

    fn targets(&self) -> &[Target] {
        &[Target::Ruchy]
    }

    fn lower(&self, module: &Module, _config: &BackendConfig) -> Result<Artifact, BackendError> {
        let primary = emit_module(module).map_err(|e| BackendError::Lower(e.to_string()))?;
        Ok(Artifact {
            primary,
            sidecars: Vec::new(),
            citations: Vec::new(),
            quorum_status: QuorumStatus::Single {
                emitter: "xpile-ruchy-codegen".to_string(),
            },
        })
    }
}

// ---------------------------------------------------------------------------
// PMAT-967: C-arithmetic emit path (the Ruchy twin of the Rust backend's C
// path). A `SourceLang::C` module lowers each `int`/`long`/`unsigned`/`double`/
// `float` function at its UNIFORM scalar width with C semantics: two's-
// complement wrapping for the integer widths (the deterministic UB-free
// discharge `C-C-INT-ARITH` commits to; DEFINED wraparound for the unsigned
// widths) and plain IEEE infix for the float widths (`C-C-FLOAT-ARITH`). The
// emitted bodies (`wrapping_add`, `i32` literal suffixes, `as u32` shift casts)
// are valid Ruchy because Ruchy compiles through Rust; only the function header
// shifts from Rust's `pub fn` to Ruchy's `fun`.
// ---------------------------------------------------------------------------

/// The uniform scalar width a C function rides. Mirrors the Rust backend's
/// `CWidth`: `rust_ty`/`lit_suffix` are the Ruchy (= Rust) type name and literal
/// suffix; `is_float` selects plain-IEEE infix over the integer `wrapping_*`.
#[derive(Clone, Copy)]
struct CWidth {
    rust_ty: &'static str,
    lit_suffix: &'static str,
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
const C_WIDTH_F32: CWidth = CWidth {
    rust_ty: "f32",
    lit_suffix: "f32",
    is_float: true,
};
// PMAT-918/921: the unsigned C widths ride `u32`/`u64`; here wrapping is the
// DEFINED C semantics for unsigned overflow (not the UB-conservative discharge
// the signed widths use).
const C_WIDTH_U32: CWidth = CWidth {
    rust_ty: "u32",
    lit_suffix: "u32",
    is_float: false,
};
const C_WIDTH_U64: CWidth = CWidth {
    rust_ty: "u64",
    lit_suffix: "u64",
    is_float: false,
};

/// "Widest wins" width pick (mirror of the Rust backend): any `f64` → `f64`;
/// else any `f32` → `f32`; else any `CULong` → `u64`; else any `CUInt` → `u32`;
/// else any `CLong` → `i64`; else `i32`.
fn c_function_width(f: &Function) -> CWidth {
    let any_f64 = matches!(f.return_type, Type::F64)
        || f.params.iter().any(|p| matches!(p.ty, Type::F64))
        || c_stmts_have_ty(&f.body.stmts, &Type::F64);
    if any_f64 {
        return C_WIDTH_F64;
    }
    let any_f32 = matches!(f.return_type, Type::F32)
        || f.params.iter().any(|p| matches!(p.ty, Type::F32))
        || c_stmts_have_ty(&f.body.stmts, &Type::F32);
    if any_f32 {
        return C_WIDTH_F32;
    }
    let any_culong = matches!(f.return_type, Type::CULong)
        || f.params.iter().any(|p| matches!(p.ty, Type::CULong))
        || c_stmts_have_ty(&f.body.stmts, &Type::CULong);
    if any_culong {
        return C_WIDTH_U64;
    }
    let any_cuint = matches!(f.return_type, Type::CUInt)
        || f.params.iter().any(|p| matches!(p.ty, Type::CUInt))
        || c_stmts_have_ty(&f.body.stmts, &Type::CUInt);
    if any_cuint {
        return C_WIDTH_U32;
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

/// Does any `let` (recursing into `while`/`if` bodies) declare a local of type
/// `want`? Drives the "widest wins" width pick (mirror of the Rust backend).
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

fn emit_c_function(out: &mut String, f: &Function) -> Result<(), RuchyCodegenError> {
    // A pointer / bare `char` param or return is meaningless on this DATA-
    // BEARING scalar-width path (pointers are an FFI-BOUNDARY concern). REFUSE
    // rather than silently render a `*mut c_int` as the function's integer width
    // — exactly the Rust backend's honest refusal.
    let has_ptr = matches!(f.return_type, Type::Ptr { .. } | Type::CChar)
        || f.params
            .iter()
            .any(|p| matches!(p.ty, Type::Ptr { .. } | Type::CChar));
    if has_ptr {
        return Err(RuchyCodegenError::Unsupported(format!(
            "C function `{}` has a pointer / `char` param or return — the C→Ruchy \
             arithmetic emit path lowers only scalar-width bodies (decy has no \
             pointer ops); a pointer boundary is an FFI-manifest shim concern",
            f.name
        )));
    }
    let w = c_function_width(f);
    if w.is_float {
        // A C `double`/`float` function obeys IEEE arithmetic, governed by
        // `C-C-FLOAT-ARITH` (the float sibling of `C-C-INT-ARITH`).
        let c_name = if w.rust_ty == "f32" {
            "float"
        } else {
            "double"
        };
        writeln!(
            out,
            "// xpile-arith: C {c_name} -> IEEE {} (governed by C-C-FLOAT-ARITH)",
            w.rust_ty
        )?;
        writeln!(out, "// xpile-contract: C-C-FLOAT-ARITH")?;
    } else {
        // C int/long/unsigned arithmetic is governed by `C-C-INT-ARITH`.
        writeln!(out, "// xpile-contract: C-C-INT-ARITH")?;
    }
    // Ruchy: `fun name(params) -> ret { body }` (no `pub`), the only surface
    // difference from the Rust backend's `pub fn`.
    write!(out, "fun {}(", f.name)?;
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

fn emit_c_stmt(
    out: &mut String,
    stmt: &Stmt,
    indent: &str,
    w: CWidth,
) -> Result<(), RuchyCodegenError> {
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
        // C early `return <expr>;` (guard clause).
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
        // C `if (c) { … } else { … }` → Ruchy if/else statement (the `else`
        // block omitted when empty).
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
        other => Err(RuchyCodegenError::Unsupported(format!(
            "C backend supports `int x = e;`, `x = e;`, `if (c) {{ … }} else {{ … }}`, and `while (c) {{ … }}`, got {other:?}"
        ))),
    }
}

fn emit_c_expr(out: &mut String, e: &Expr, w: CWidth) -> Result<(), RuchyCodegenError> {
    match e {
        // The literal suffix tracks the function width so the body is
        // internally type-consistent. An int literal in a float-width function
        // emits as `<v>f64`/`<v>f32`.
        Expr::LitInt(v) => write!(out, "{v}{}", w.lit_suffix)?,
        Expr::LitFloat(v) => {
            let suffix = if w.is_float { w.lit_suffix } else { "f64" };
            write!(out, "{v}{suffix}")?
        }
        Expr::Ident(name) => write!(out, "{name}")?,
        Expr::BinOp { op, lhs, rhs } => emit_c_binop(out, *op, lhs, rhs, w)?,
        Expr::UnOp { op, operand } => match op {
            // C unary minus on an integer is wrapping (INT_MIN negation is UB
            // in C; `wrapping_neg` is the sound deterministic discharge). On a
            // float it is plain IEEE negation `-(x)`.
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
            return Err(RuchyCodegenError::Unsupported(format!(
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
) -> Result<(), RuchyCodegenError> {
    // Arithmetic: wrapping (C signed overflow is UB → deterministic two's-
    // complement; unsigned wrap is DEFINED). Comparisons / logicals: plain
    // infix producing a `bool`. The `wrapping_*` methods are width-agnostic in
    // syntax (they wrap at the operand's width).
    let wrapping = |out: &mut String, method: &str| -> Result<(), RuchyCodegenError> {
        write!(out, "(")?;
        emit_c_expr(out, lhs, w)?;
        write!(out, ").{method}(")?;
        emit_c_expr(out, rhs, w)?;
        write!(out, ")")?;
        Ok(())
    };
    let infix = |out: &mut String, sym: &str| -> Result<(), RuchyCodegenError> {
        emit_c_expr(out, lhs, w)?;
        write!(out, " {sym} ")?;
        emit_c_expr(out, rhs, w)?;
        Ok(())
    };
    // A FULLY-parenthesized infix `(lhs OP rhs)` for the C bitwise operators —
    // the parens pin the C-intended grouping against Rust's different native
    // bitwise/comparison precedence.
    let paren_infix = |out: &mut String,
                       lhs: &Expr,
                       sym: &str,
                       rhs: &Expr,
                       w: CWidth|
     -> Result<(), RuchyCodegenError> {
        write!(out, "(")?;
        emit_c_expr(out, lhs, w)?;
        write!(out, " {sym} ")?;
        emit_c_expr(out, rhs, w)?;
        write!(out, ")")?;
        Ok(())
    };
    // A C shift `(lhs).wrapping_shl((rhs) as u32)` — `wrapping_shl`/
    // `wrapping_shr` mask the shift distance to the operand bit width, so the
    // result is TOTAL and UB-free. The std signature takes the shift amount as
    // `u32`, hence the `as u32` cast.
    let wrapping_shift = |out: &mut String,
                          lhs: &Expr,
                          method: &str,
                          rhs: &Expr,
                          w: CWidth|
     -> Result<(), RuchyCodegenError> {
        write!(out, "(")?;
        emit_c_expr(out, lhs, w)?;
        write!(out, ").{method}((")?;
        emit_c_expr(out, rhs, w)?;
        write!(out, ") as u32)")?;
        Ok(())
    };
    match op {
        // On a float width, C arithmetic is plain IEEE infix (`+ - * / %`) —
        // f64/f32 have no `wrapping_*` and never wrap.
        BinOp::Add if w.is_float => infix(out, "+")?,
        BinOp::Sub if w.is_float => infix(out, "-")?,
        BinOp::Mul if w.is_float => infix(out, "*")?,
        BinOp::FloorDiv if w.is_float => infix(out, "/")?,
        BinOp::Mod if w.is_float => infix(out, "%")?,
        BinOp::Add => wrapping(out, "wrapping_add")?,
        BinOp::Sub => wrapping(out, "wrapping_sub")?,
        BinOp::Mul => wrapping(out, "wrapping_mul")?,
        // C `/` truncates toward zero (Rust integer `/` does too);
        // `wrapping_div`/`wrapping_rem` add the INT_MIN/-1 UB guard. The
        // frontend carries C truncating div/rem as FloorDiv/Mod (shared IR
        // variants), NOT Python floor.
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
        // C bitwise `& | ^` are integer-only — invalid on a float operand (a C
        // type error). Refuse on the float widths rather than mis-emit.
        BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor if w.is_float => {
            return Err(RuchyCodegenError::Unsupported(format!(
                "C bitwise operator BinOp::{op:?} is not valid on the float width \
                 `{}` (bitwise ops require an integer operand in C)",
                w.rust_ty
            )));
        }
        BinOp::BitAnd => paren_infix(out, lhs, "&", rhs, w)?,
        BinOp::BitOr => paren_infix(out, lhs, "|", rhs, w)?,
        BinOp::BitXor => paren_infix(out, lhs, "^", rhs, w)?,
        // C shift `<< >>` → `wrapping_shl`/`wrapping_shr` (total, UB-free).
        BinOp::Shl | BinOp::Shr if w.is_float => {
            return Err(RuchyCodegenError::Unsupported(format!(
                "C shift operator BinOp::{op:?} is not valid on the float width \
                 `{}` (shift requires an integer operand in C)",
                w.rust_ty
            )));
        }
        BinOp::Shl => wrapping_shift(out, lhs, "wrapping_shl", rhs, w)?,
        BinOp::Shr => wrapping_shift(out, lhs, "wrapping_shr", rhs, w)?,
        other => {
            return Err(RuchyCodegenError::Unsupported(format!(
                "C backend slice 1 does not lower BinOp::{other:?} — power is \
                 deferred to a later decy slice"
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
    fn emits_fun_keyword_not_pub_fn() {
        let m = module_with("fixture", vec![Item::Function(add_fn())]);
        let ruchy = emit_module(&m).expect("emit ok");
        assert!(
            ruchy.contains("fun add("),
            "Ruchy uses `fun`, not `fn` or `pub fn`: got\n{}",
            ruchy
        );
        assert!(
            !ruchy.contains("pub fn"),
            "Ruchy emission must not produce `pub fn` (that's Rust)"
        );
        // Post PMAT-002: addition lowers to checked_add (Ruchy compiles
        // to Rust, so it shares Rust's overflow semantics + contract
        // C-PY-INT-ARITH).
        assert!(
            ruchy.contains("checked_add"),
            "expected checked_add: {ruchy}"
        );
        assert!(ruchy.contains("C-PY-INT-ARITH"));
    }

    #[test]
    fn ruchy_floordiv_uses_floor_correction() {
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
        let ruchy = emit_module(&m).expect("emit ok");
        // PMAT-538: floor correction, not div_euclid (wrong for a neg divisor).
        assert!(ruchy.contains("checked_div") && ruchy.contains("__q - 1"));
        assert!(!ruchy.contains("div_euclid"));
    }

    // -- PMAT-967: C-arithmetic Ruchy emit path (parity with the Rust backend) --

    fn c_module(name: &str, items: Vec<Item>) -> Module {
        Module {
            name: name.into(),
            source_lang: SourceLang::C,
            items,
            ffi_boundaries: Vec::new(),
        }
    }

    /// `int <name>(int a, int b) { return a OP b; }` — the canonical C scalar
    /// fixture decy produces, parameterised on the binary operator.
    fn c_int_binop_fn(name: &str, op: BinOp) -> Function {
        Function {
            name: name.into(),
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
                    op,
                    lhs: Box::new(Expr::Ident("a".into())),
                    rhs: Box::new(Expr::Ident("b".into())),
                },
            },
        }
    }

    #[test]
    fn c_int_add_emits_wrapping_i32_not_checked_i64() {
        // BEFORE PMAT-967 this routed through `emit_function` and emitted the
        // Python `checked_add` / i64 path — a mis-emit for C's wrapping i32.
        let m = c_module(
            "fixture",
            vec![Item::Function(c_int_binop_fn("add", BinOp::Add))],
        );
        let ruchy = emit_module(&m).expect("emit ok");
        // C arithmetic: two's-complement wrapping at the i32 width, NOT the
        // panic-on-overflow checked path the Python/Ruchy default uses.
        assert!(
            ruchy.contains("(a).wrapping_add(b)"),
            "expected wrapping_add: got\n{ruchy}"
        );
        assert!(
            !ruchy.contains("checked_add"),
            "C path must NOT emit the checked (panic) path: got\n{ruchy}"
        );
        // Width: the i32 function signature, not i64.
        assert!(
            ruchy.contains("fun add(a: i32, b: i32) -> i32"),
            "expected i32-width `fun` header: got\n{ruchy}"
        );
        // Ruchy header, not Rust's `pub fn`.
        assert!(!ruchy.contains("pub fn"));
        // Cites the C integer-arithmetic contract.
        assert!(
            ruchy.contains("// xpile-contract: C-C-INT-ARITH"),
            "expected C-C-INT-ARITH citation: got\n{ruchy}"
        );
    }

    #[test]
    fn c_int_sub_mul_div_mod_are_wrapping() {
        for (op, method) in [
            (BinOp::Sub, "wrapping_sub"),
            (BinOp::Mul, "wrapping_mul"),
            (BinOp::FloorDiv, "wrapping_div"),
            (BinOp::Mod, "wrapping_rem"),
        ] {
            let m = c_module("fixture", vec![Item::Function(c_int_binop_fn("op", op))]);
            let ruchy = emit_module(&m).expect("emit ok");
            assert!(
                ruchy.contains(&format!("(a).{method}(b)")),
                "expected {method} for {op:?}: got\n{ruchy}"
            );
        }
    }

    #[test]
    fn c_long_function_rides_i64_wrapping() {
        let mut f = c_int_binop_fn("addl", BinOp::Add);
        f.params[0].ty = Type::CLong;
        f.params[1].ty = Type::CLong;
        f.return_type = Type::CLong;
        let ruchy = emit_module(&c_module("fixture", vec![Item::Function(f)])).expect("emit ok");
        assert!(
            ruchy.contains("fun addl(a: i64, b: i64) -> i64"),
            "expected i64-width header: got\n{ruchy}"
        );
        assert!(ruchy.contains("(a).wrapping_add(b)"));
    }

    #[test]
    fn c_unsigned_function_rides_u32_wrapping() {
        let mut f = c_int_binop_fn("addu", BinOp::Add);
        f.params[0].ty = Type::CUInt;
        f.params[1].ty = Type::CUInt;
        f.return_type = Type::CUInt;
        let ruchy = emit_module(&c_module("fixture", vec![Item::Function(f)])).expect("emit ok");
        assert!(
            ruchy.contains("fun addu(a: u32, b: u32) -> u32"),
            "expected u32-width header: got\n{ruchy}"
        );
        assert!(ruchy.contains("(a).wrapping_add(b)"));
    }

    #[test]
    fn c_double_function_uses_ieee_infix_and_float_contract() {
        let mut f = c_int_binop_fn("addd", BinOp::Add);
        f.params[0].ty = Type::F64;
        f.params[1].ty = Type::F64;
        f.return_type = Type::F64;
        let ruchy = emit_module(&c_module("fixture", vec![Item::Function(f)])).expect("emit ok");
        // IEEE infix `+`, NOT a wrapping method (f64 has no `wrapping_*`).
        assert!(
            ruchy.contains("fun addd(a: f64, b: f64) -> f64"),
            "expected f64-width header: got\n{ruchy}"
        );
        assert!(ruchy.contains("a + b"));
        assert!(!ruchy.contains("wrapping"));
        assert!(
            ruchy.contains("// xpile-contract: C-C-FLOAT-ARITH"),
            "double fn cites the IEEE float contract: got\n{ruchy}"
        );
    }

    #[test]
    fn c_bitwise_and_shift_lower() {
        let m = c_module(
            "fixture",
            vec![
                Item::Function(c_int_binop_fn("band", BinOp::BitAnd)),
                Item::Function(c_int_binop_fn("shl", BinOp::Shl)),
            ],
        );
        let ruchy = emit_module(&m).expect("emit ok");
        // Bitwise: fully-parenthesised infix to pin C grouping.
        assert!(ruchy.contains("(a & b)"), "expected (a & b): got\n{ruchy}");
        // Shift: wrapping_shl with the `as u32` distance cast.
        assert!(
            ruchy.contains("(a).wrapping_shl((b) as u32)"),
            "expected wrapping_shl((..) as u32): got\n{ruchy}"
        );
    }

    #[test]
    fn c_if_while_body_lowers() {
        // `int clamp(int a) { if (a < 0) { return 0; } return a; }`
        let f = Function {
            name: "clamp".into(),
            params: vec![Param {
                name: "a".into(),
                ty: Type::I64,
                mutable: false,
            }],
            return_type: Type::I64,
            body: Block {
                stmts: vec![Stmt::If {
                    cond: Expr::BinOp {
                        op: BinOp::Lt,
                        lhs: Box::new(Expr::Ident("a".into())),
                        rhs: Box::new(Expr::LitInt(0)),
                    },
                    then_body: vec![Stmt::Return(Expr::LitInt(0))],
                    else_body: vec![],
                }],
                trailing_return: Expr::Ident("a".into()),
            },
        };
        let ruchy = emit_module(&c_module("fixture", vec![Item::Function(f)])).expect("emit ok");
        // The int literal carries the i32 width suffix (`0i32`), keeping the body
        // internally type-consistent — same as the Rust backend's C path.
        assert!(ruchy.contains("if a < 0i32 {"), "got\n{ruchy}");
        assert!(ruchy.contains("return 0i32;"), "got\n{ruchy}");
    }

    #[test]
    fn c_pointer_function_is_refused_not_misemitted() {
        let f = Function {
            name: "deref".into(),
            params: vec![Param {
                name: "p".into(),
                ty: Type::Ptr {
                    mutable: false,
                    pointee: Box::new(Type::I64),
                },
                mutable: false,
            }],
            return_type: Type::I64,
            body: Block {
                stmts: vec![],
                trailing_return: Expr::LitInt(0),
            },
        };
        let err = emit_module(&c_module("fixture", vec![Item::Function(f)]))
            .expect_err("a pointer C function must be refused, not mis-emitted");
        assert!(
            matches!(err, RuchyCodegenError::Unsupported(_)),
            "expected honest Unsupported refusal, got {err:?}"
        );
    }
}
