# PMAT-455 / v0.2.0 Track 1.B foundation: list[int] literal returning
# function. Exercises Type::List(Box<Type::I64>) + Expr::ListLit
# end-to-end. Governing contract: C-XLATE-PY-LIST-TO-VEC.
def squares() -> list[int]:
    return [1, 4, 9, 16, 25]
