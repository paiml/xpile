# PMAT-461 / v0.2.0 Track 1.B: indexed assignment `xs[i] = v`.
# Exercises Stmt::IndexAssign with param-mut threading. Verifies that
# in-place mutation via subscript-assignment works alongside the
# matching Expr::Index read.
def set_first(xs: list[int], v: int) -> int:
    xs[0] = v
    return xs[0]
