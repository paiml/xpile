# PMAT-477 (R8): Python float (f64). Float arithmetic is plain infix
# (IEEE-754, no checked/overflow); `/` is true division (not floor).
def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def average(x: float, y: float) -> float:
    return (x + y) / 2.0


def scale(xs: float, k: float) -> float:
    r = xs * k
    return r
