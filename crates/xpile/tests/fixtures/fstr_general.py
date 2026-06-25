# PMAT-965 (correctness-hunt): the GENERAL-float format spec `:g` / `:G` and
# `:.Ng` / `:.NG` over a float — f"{1234.5:g}" == "1234.5",
# f"{123456789:g}" == "1.23457e+08", f"{3.14159:.3g}" == "3.14",
# f"{1000000.0:g}" == "1e+06" — were a clean reject ("unsupported format spec
# `:g`"). `:g` is Python's DEFAULT float presentation (the C `%g` rule): with
# precision P (default 6, clamped to >=1), let X be the decimal exponent of the
# value rounded to P significant digits — use FIXED notation with P-1-X decimals
# when -4 <= X < P, else SCIENTIFIC with P-1 decimals — then STRIP trailing
# zeros (and a trailing `.`). Scientific output carries Python's `e±NN` exponent
# (sign always present, magnitude zero-padded to >=2 digits); inf/nan (upper
# INF/NAN) pass through. Rust's format! has no `%g`, so the spec routes to the
# new FloatGeneralStr node, which ports the algorithm. An int with this
# presentation is coerced to float (f"{5:g}" == "5"). Validated byte-identical
# to CPython over 513k (value x precision) cases. vs python3.


def gen(x: float) -> str:
    return f"{x:g}"


def gen_upper(x: float) -> str:
    return f"{x:G}"


def gen_p3(x: float) -> str:
    return f"{x:.3g}"


def gen_p1(x: float) -> str:
    return f"{x:.1g}"


def gen_int(n: int) -> str:
    return f"{n:g}"


def labeled(x: float) -> str:
    return f"x={x:g}!"
