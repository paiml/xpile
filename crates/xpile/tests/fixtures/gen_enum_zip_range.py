# PMAT-772 (HUNT-V16 GEN-01/02): `list(enumerate(range(n)))` and
# `list(zip(range(n), xs))` emitted raw `enumerate`/`zip`/`range`/`list` free
# calls (rustc E0425) — a bare `range(...)` isn't a first-class value, so the
# enumerate/zip arg wasn't materialized. The enumerate/zip expression lowering
# now materializes a range arg into a Vec (reusing the len()/sum() machinery).
# Cross-checked vs python3.


def enum_len() -> int:
    pairs = list(enumerate(range(4)))
    return len(pairs)


def zip_len() -> int:
    pairs = list(zip(range(3), [10, 20, 30]))
    return len(pairs)


def zip3_len() -> int:
    pairs = list(zip(range(2), [1, 2], [3, 4]))
    return len(pairs)
