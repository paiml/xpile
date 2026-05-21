# PMAT-462 / v0.2.0 Track 1.B: nested list[list[int]] literal. Verifies
# that recursive Type::List and Expr::ListLit compose cleanly. Also
# catches the Lean parenthesization fix — without parens around the
# inner List elem, Lean parses `List List Int` as application of two
# distinct types and fails to typecheck.
def grid() -> list[list[int]]:
    return [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
