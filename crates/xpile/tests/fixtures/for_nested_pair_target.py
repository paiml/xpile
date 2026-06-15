# PMAT-705: a nested-tuple `for`-pair target — `for i, (a, b) in enumerate(xs)`,
# `for k, (a, b) in d.items()` — was rejected ("non-Name for target"). Desugared
# to a fresh temp + a prepended `(a, b) = __temp` unpack, reusing the pair-loop +
# tuple-unpack machinery. A nested var may be mutated.
def enum_nested(xs: list[tuple[int, int]]) -> int:
    total = 0
    for i, (a, b) in enumerate(xs):
        total += i + a + b
    return total


def items_nested(d: dict[str, tuple[int, int]]) -> int:
    total = 0
    for k, (a, b) in d.items():
        total += a + b
    return total


def mutate_nested(xs: list[tuple[int, int]]) -> int:
    total = 0
    for i, (a, b) in enumerate(xs):
        a += 10
        total += a + b
    return total
