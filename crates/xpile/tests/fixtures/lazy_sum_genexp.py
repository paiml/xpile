# PMAT-1099 (sum follow-up): sum() over a range-sourced generator expression must
# STREAM (lazy `(a..b).map(..).fold(..)`), not collect the range into a Vec. The
# old emit double-materialized (range → Vec, map → Vec) → `sum(f(x) for x in
# range(10**11))` OOMed; CPython streams. Recurses through map + filter clauses.
def sum_mapped() -> int:
    return sum(x * 2 for x in range(10))


def sum_plain() -> int:
    return sum(x for x in range(100))


def sum_filtered() -> int:
    return sum(x for x in range(20) if x % 2 == 0)
