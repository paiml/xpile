# PMAT-835 (HUNT-V26 #15): str(range(...)) is Python's range repr ("range(0, 5)",
# "range(2, 10, 3)"), but xpile materialized the range to a list and rendered
# "[0, 1, 2, 3, 4]" (silent-wrong). The str() builtin now matches range(...)
# syntactically and builds the repr from its 1..=3 args. Cross-checked vs python3.


def one() -> str:
    return str(range(5))


def two() -> str:
    return str(range(2, 10))


def three() -> str:
    return str(range(2, 20, 3))


def with_var(n: int) -> str:
    return str(range(n))
