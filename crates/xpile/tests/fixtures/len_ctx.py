# PMAT-502w (Tranche 2): ctx-aware len() over context-dependent expressions
# (dict views, sorted, ...). Previously len(d.keys()) was a hard error.
def num_keys(d: dict[int, int]) -> int:
    return len(d.keys())


def num_values(d: dict[int, int]) -> int:
    return len(d.values())


def len_sorted(xs: list[int]) -> int:
    return len(sorted(xs))
