# PMAT-798 (HUNT-V19 ND-04): an N-arity (>=3) tuple for-target over a
# list[tuple[...]] (`for a, b, c in triples`) was rejected ("non-Name for
# target"), while the identical 2-element form transpiles. The for-target was
# hard-capped to arity 2; arity 3+ now desugars to a fresh single loop var + a
# tuple-unpack assignment (which already supports any arity). Cross-checked vs
# python3.


def sum_triples() -> int:
    triples: list[tuple[int, int, int]] = [(1, 2, 3), (4, 5, 6)]
    total: int = 0
    for a, b, c in triples:
        total += a + b + c
    return total


def use4() -> int:
    rows: list[tuple[int, int, int, int]] = [(1, 2, 3, 4), (10, 20, 30, 40)]
    t: int = 0
    for a, b, c, d in rows:
        t += a * b + c - d
    return t
