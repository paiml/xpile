# PMAT-681: bool(x) over a float is `x != 0.0` (0.0 / -0.0 falsy, NaN/inf truthy).
# Was rejected ("float deferred"); the implicit `if x:` path already did this.
def is_nonzero(x: float) -> bool:
    return bool(x)
