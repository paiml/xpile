# PMAT-700: a plain (no-star) tuple-unpack over a LIST — `a, b = xs` — now
# transpiles (it was rejected "expected a tuple"). Python unpacks by position
# with an exact-length check; xpile emits a length assert + `let a = xs[0]; …`.
def two(xs: list[int]) -> int:
    a, b = xs
    return a + b


def three(xs: list[str]) -> str:
    a, b, c = xs
    return a + c


def from_list_lit(n: int) -> int:
    a, b = [n, n * 2]
    return a + b
