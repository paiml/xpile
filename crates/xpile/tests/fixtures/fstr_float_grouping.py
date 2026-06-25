# PMAT-940 (correctness-hunt): the thousands-GROUPING spec on a FLOAT with a
# fixed precision — `f"{1234567.89:,.2f}"` == "1,234,567.89",
# `f"{1234567.5:_.1f}"` == "1_234_567.5", and the default-precision `,f`/`_f`
# (== Python's 6 decimals) — were a clean reject ("unsupported format spec
# `:,`"). Python renders the float to N decimals, then groups the integer part's
# digits by 3 from the right with the separator, sign FIRST for negatives. An
# int with a float-presentation spec is coerced to float (`f"{1234567:,.2f}"` ==
# "1,234,567.00"). Rust's format! has no grouping flag, so the spec routes to the
# new FloatGroupedStr digit-grouping loop (integer part only). vs python3.


def grp_comma2(x: float) -> str:
    return f"{x:,.2f}"


def grp_under1(x: float) -> str:
    return f"{x:_.1f}"


def grp_default(x: float) -> str:
    return f"{x:,f}"


def grp_int(n: int) -> str:
    return f"{n:,.2f}"


def labeled(x: float) -> str:
    return f"bal={x:,.2f}!"
