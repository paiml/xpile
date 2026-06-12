# PMAT-502x (Tranche 2): d.items() -> list of (k, v) tuples; composes with
# sorted (tuples are Ord) and len (ctx-aware, v0.1.55).
def sorted_items(d: dict[int, int]) -> list[tuple[int, int]]:
    return sorted(d.items())


def num_items(d: dict[int, int]) -> int:
    return len(d.items())
