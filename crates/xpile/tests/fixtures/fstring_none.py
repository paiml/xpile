# PMAT-1168: a literal `None` interpolated in an f-string renders the string
# "None" (CPython: `str(None)` == `repr(None)` == "None", so `f"{None}"` is
# "None"). A bare `None` lowered to `Expr::OptionExpr(None)` — Rust `None`, which
# has no `Display` — so `format!("{}", None)` was rustc E0277 (accept-then-fail:
# invalid Rust). Normal f-strings are unaffected.
def none_mid() -> str:
    return f"x{None}y"


def none_labeled(n: int) -> str:
    return f"val={None} n={n}"


def none_lone() -> str:
    return f"{None}"
