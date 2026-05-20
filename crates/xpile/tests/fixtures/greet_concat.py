# PMAT-451 / v0.2.0 Track 1.A: str + str concatenation via Expr::Concat.
# `"hello, " + name` lowers to Expr::Concat which emits `format!("{}{}", lhs, rhs)`
# in Rust/Ruchy and `lhs ++ rhs` in Lean. Governing equation:
# C-XLATE-PY-STR-TO-RUST-STRING::concatenation_associativity.
def greet(name: str) -> str:
    return "hello, " + name
