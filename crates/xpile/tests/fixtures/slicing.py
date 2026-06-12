# PMAT-496 (sprint): bounded slicing xs[a:b] for list and str.
# list slice -> Vec (owned via .to_vec()); str slice -> String (byte-
# indexed, ASCII-correct). Open-ended / step / negative are deferred.
def middle(xs: list[int]) -> list[int]:
    return xs[1:3]


def prefix(s: str) -> str:
    return s[0:3]
