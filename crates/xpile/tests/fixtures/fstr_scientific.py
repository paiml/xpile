# PMAT-941 (correctness-hunt): the SCIENTIFIC-NOTATION format spec `:e` / `:E`
# and `:.Ne` / `:.NE` over a float — f"{1234.5:e}" == "1.234500e+03",
# f"{1234.5:.2E}" == "1.23E+03", f"{1e100:e}" == "1.000000e+100" (bare `e`/`E`
# defaults to 6 decimals) — were a clean reject ("unsupported format spec `:e`").
# Rust's format!("{:.Ne}", x) renders the right mantissa but a BARE exponent
# (`1.234500e3` — no sign, no 2-digit-min zero-pad) and lowercases inf/nan even
# under `{:E}`, so the spec routes to the new FloatSciStr node, which renders
# then fixes up the exponent to Python's `e±NN` form and case-folds the
# non-finite tail. An int with this presentation is coerced to float
# (f"{5:e}" == "5.000000e+00"). vs python3.


def sci(x: float) -> str:
    return f"{x:e}"


def sci_upper(x: float) -> str:
    return f"{x:E}"


def sci_p2(x: float) -> str:
    return f"{x:.2e}"


def sci_p0_upper(x: float) -> str:
    return f"{x:.0E}"


def sci_int(n: int) -> str:
    return f"{n:e}"


def labeled(x: float) -> str:
    return f"v={x:.3e}!"
