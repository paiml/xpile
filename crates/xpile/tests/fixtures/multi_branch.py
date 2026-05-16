# Multi-statement / multi-assignment if-branches (PMAT-005).
#
# Both branches assign `lo` AND `hi`. Pre-PMAT-005 the frontend rejected
# this. Post-PMAT-005 each name gets its own `let = if cond { ... } else { ... }`.
#
# `range_size(a, b)` returns |a - b| via min-max sorting.
def range_size(a: int, b: int) -> int:
    if a < b:
        lo = a
        hi = b
    else:
        lo = b
        hi = a
    return hi - lo
