# PMAT-1170: a literal `None` interpolated in an f-string renders the string
# "None" (CPython: `str(None)` == `repr(None)` == "None", so `f"{None}"` is
# "None"). A bare `None` lowered to `Expr::OptionExpr(None)` whose inner type is
# un-inferable — the Optional path desugared it to `(None).unwrap()`, which rustc
# rejected with E0282 (accept-then-fail: transpile-success → invalid Rust).
# Normal f-strings are unaffected. Distinct from #1765's Optional-VALUE handling.
def none_mid() -> str:
    return f"x{None}y"


def none_labeled(n: int) -> str:
    return f"val={None} n={n}"


def none_lone() -> str:
    return f"{None}"
