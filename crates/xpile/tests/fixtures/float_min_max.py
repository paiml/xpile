# PMAT-601: 2-arg max()/min() over floats must follow Python's first-argument-
# wins semantics, not f64::max/min (which treat +0.0 as > -0.0 and silently drop
# NaN). On a tie / incomparable compare Python keeps the first argument.
def mx(a: float, b: float) -> float:
    return max(a, b)


def mn(a: float, b: float) -> float:
    return min(a, b)


def mx3(a: float, b: float, c: float) -> float:
    return max(a, b, c)
