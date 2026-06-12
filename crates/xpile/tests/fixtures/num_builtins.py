# PMAT-498 (Tranche 2): scalar numeric builtins abs / min / max.
# abs(x) -> (x).abs(); min(a,b) -> (a).min(b); max(a,b) -> (a).max(b).
def clamp(x: int, lo: int, hi: int) -> int:
    return min(max(x, lo), hi)


def magnitude(x: int) -> int:
    return abs(x)
