# PMAT-1152 (oracle-reliability): Python sets lower to indexmap::IndexSet (not
# std HashSet) so iteration order is DETERMINISTIC run-to-run — HashSet's
# per-process RandomState made set-order output flap (non-deterministic Rust,
# thrashing the differential oracle). Order-independent set ops are unaffected.
def dedup_count(xs: list[int]) -> int:
    return len(set(xs))


def union_size(a: list[int], b: list[int]) -> int:
    sa = set(a)
    sb = set(b)
    return len(sa | sb)


def membership(x: int) -> bool:
    s = {1, 2, 3, 5, 8}
    return x in s
