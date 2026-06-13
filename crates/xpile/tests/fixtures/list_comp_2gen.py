def cartesian(a: list[int], b: list[int]) -> list[int]:
    # Two generators → nested loops (products).
    return [x * y for x in a for y in b]


def filtered(a: list[int], b: list[int]) -> list[int]:
    # Per-generator `if` filters attach to their own loop.
    return [x + y for x in a if x > 0 for y in b if y < 10]


def pairs(a: list[int], b: list[int]) -> list[int]:
    # Assignment position (not just return) also desugars.
    out = [x - y for x in a for y in b]
    return out
