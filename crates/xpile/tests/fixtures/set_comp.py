# PMAT-501b (Tranche 2): set comprehension {e for x in xs}.
# Materialises to s = set(); for x in xs: s.add(e).
def distinct_doubles(xs: list[int]) -> int:
    s = {x * 2 for x in xs}
    return len(s)


def has_square(xs: list[int], q: int) -> bool:
    squares = {x * x for x in xs}
    return q in squares
