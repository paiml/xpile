# PMAT-1099: any()/all() over a generator expression sourced from range(...) must
# be LAZY (short-circuit), not materialize the range into a Vec. The old emit
# `(0..N).collect::<Vec>().iter().cloned().any(..)` collected the whole range —
# `any(p for x in range(10**11))` attempted an 800GB alloc where CPython stops at
# the first hit. Now emits `(0..N).any(..)` / `(0..N).filter(..).any(..)`.
def any_over_range() -> bool:
    return any(x > 3 for x in range(10))


def all_over_range() -> bool:
    return all(x >= 0 for x in range(10))


def any_filtered() -> bool:
    return any(x > 15 for x in range(20) if x % 2 == 0)
