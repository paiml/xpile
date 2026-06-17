# PMAT-750 (HUNT-V14 #6 dc-order-sort-no-ord): `@dataclass(order=True)` derived
# only `PartialOrd`, but `list.sort()` / `sorted()` require `Ord` → rustc E0277
# (transpile-success → invalid Rust). When every field is Ord-able
# (int/bool/str) the dataclass now also derives `Ord` (+ `Eq`), so instances
# sort by field order (Python tuple comparison). A float field can't derive
# `Ord` (f64 is not `Ord`), so a float-field order=True dataclass keeps
# `PartialOrd` only (comparisons still work; sorting it stays deferred).
# Cross-checked vs python3.

from dataclasses import dataclass


@dataclass(order=True)
class P:
    x: int
    y: int


def first_after_sort() -> int:
    ps = [P(2, 1), P(1, 9), P(1, 2)]
    ps.sort()
    # lexicographic: (1,2) < (1,9) < (2,1) → first is P(1, 2) → y == 2
    return ps[0].y


def sorted_builtin() -> int:
    ps = [P(3, 0), P(1, 0), P(2, 0)]
    out = sorted(ps)
    return out[0].x


def compare_still_works() -> int:
    # the PartialOrd comparison path (PMAT-648) is unchanged
    if P(1, 2) < P(1, 3):
        return 1
    return 0
