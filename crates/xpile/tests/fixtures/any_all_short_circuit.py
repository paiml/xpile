# PMAT-689: any()/all() over a GENERATOR expression must short-circuit like
# Python's lazy genexpr — `any(P(x) for x in xs)` was eager-mapped over ALL
# elements then reduced, panicking on a not-yet-needed element (e.g. div-by-zero)
# Python never reaches. A LIST comprehension `any([...])` is eager in Python, so
# it is NOT fused (stays eager).
def has_big(xs: list[int]) -> bool:
    return any(1000 // x > 5 for x in xs)


def all_short(xs: list[int]) -> bool:
    return all(x > 0 and 100 // x > 0 for x in xs)


def any_truthy(xs: list[int]) -> bool:
    return any(x for x in xs)


def listcomp_eager(xs: list[int]) -> bool:
    return any([x > 5 for x in xs])
