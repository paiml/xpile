# PMAT-456 / v0.2.0 Track 1.B: list[str] literal. Exercises
# Type::List(Box<Type::Str>) + Expr::ListLit containing Expr::LitStr
# elements. Governing contract: C-XLATE-PY-LIST-TO-VEC, with element
# type Str governed by C-XLATE-PY-STR-TO-RUST-STRING.
def names() -> list[str]:
    return ["alice", "bob", "carol"]
