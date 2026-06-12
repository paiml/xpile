# PMAT-502af (Tranche 2): str(x) over a float -> Python-matching string
# (whole numbers get a ".0" suffix, unlike Rust's bare format!).
def f_str(x: float) -> str:
    return str(x)


def half_str(n: int) -> str:
    return str(float(n) / 2.0)
