# PMAT-680: a float-precision format spec over NaN must print "nan" (Python),
# not Rust's "NaN". The f-string path already guarded this; `.format()` and `%`
# inlined the spec into a bare format!, bypassing the guard.
def via_format(x: float) -> str:
    return "{:.2f}".format(x)


def via_percent(x: float) -> str:
    return "%.2f" % x


def pct_default(x: float) -> str:
    return "%f" % x


def via_fstring(x: float) -> str:
    return f"{x:.2f}"


def normal(x: float) -> str:
    return "{:.2f}".format(x)
