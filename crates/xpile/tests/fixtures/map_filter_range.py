# PMAT-775 (HUNT-V16 GEN-03): `list(map(lambda v: v*2, range(n)))` and
# `list(filter(lambda v: ..., range(n)))` were rejected with a misleading
# "lambda stored in a general expression" message — the map/filter iterable
# argument was lowered plainly, so a bare `range(...)` (not a first-class value)
# didn't type as a list and the lambda path was skipped. The map/filter lowering
# now materializes a range arg into a Vec (same as PMAT-772 for enumerate/zip).
# Cross-checked vs python3.


def map_lambda_range() -> int:
    return sum(list(map(lambda v: v * 2, range(4))))


def filter_lambda_range() -> int:
    return sum(list(filter(lambda v: v % 2 == 0, range(6))))


def map_bare_callable_range() -> int:
    return sum(list(map(abs, range(3))))
