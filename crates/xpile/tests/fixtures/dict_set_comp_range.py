# PMAT-502bd (Tranche 2): dict + set comprehensions over range(...).
def sq_map(n: int) -> dict[int, int]:
    return {x: x * x for x in range(n)}


def even_set(n: int) -> set[int]:
    return {x for x in range(n) if x > 0}


def from_two(n: int) -> int:
    d = {x: x for x in range(2, n)}
    return len(d)
