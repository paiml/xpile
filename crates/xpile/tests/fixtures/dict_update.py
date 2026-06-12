# PMAT-502bb (Tranche 2): in-place dict merge a.update(b).
def merge(a: dict[str, int], b: dict[str, int]) -> int:
    a.update(b)
    return len(a)


def merge_local(b: dict[str, int]) -> int:
    a: dict[str, int] = {}
    a.update(b)
    return len(a)
