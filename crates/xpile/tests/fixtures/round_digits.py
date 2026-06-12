# PMAT-502al (Tranche 2): round(x, n) -> float rounded to n decimals via
# banker's rounding after 10^n scaling, matching Python's float-repr behavior.
def r2(x: float, n: int) -> float:
    return round(x, n)


def half_cent(x: float) -> float:
    return round(x, 2)
