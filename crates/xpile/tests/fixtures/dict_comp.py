# PMAT-501 (Tranche 2): dict comprehension {k: v for x in xs}.
# Materialises like a list comp: acc = {}; for x in xs: acc[k] = v.
def squares(xs: list[int]) -> dict[int, int]:
    return {x: x * x for x in xs}


def lengths(words: list[str]) -> dict[str, int]:
    result = {w: len(w) for w in words}
    return result
