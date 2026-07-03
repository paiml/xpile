# PMAT-1085 (finding c): pair-loops were entirely outside the PMAT-1080
# mut-gating fix — both directions.

# An OUTER var reassigned inside a pair-loop body: the reassignment scan had
# no ForEachPair arm, so the outer `for x` binding got no `mut` (E0384).
# CPython: (1+5+6) + (2+5+6) + (3+5+6) = 39.
def outer_in_pair() -> int:
    total: int = 0
    for x in [1, 2, 3]:
        for i, y in enumerate([5, 6]):
            x = x + y
        total = total + x
    return total


# The pair-loop's OWN tuple binding reassigned: the emission had no mut
# gating at all (E0384). CPython: 10 + 12 + 14 = 36.
def pair_binding() -> int:
    total: int = 0
    for i, y in enumerate([5, 6, 7]):
        y = y * 2
        total = total + y
    return total


# Same for a zip3 tuple binding. CPython: (1+13+5) + (2+14+6) = 41.
def zip3_binding() -> int:
    total: int = 0
    for a, b, c in zip([1, 2], [3, 4], [5, 6]):
        b = b + 10
        total = total + a + b + c
    return total
