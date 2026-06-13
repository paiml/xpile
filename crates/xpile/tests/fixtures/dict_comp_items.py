# PMAT-502cf (Tranche 2): dict comprehension over `d.items()` with a tuple
# target `{k: f(v) for k, v in d.items()}` — desugars to a ForEachPair(Pairs)
# loop building the dict. The optional `if` filter composes.
def doubled(d: dict[str, int]) -> dict[str, int]:
    return {k: v * 2 for k, v in d.items()}


def positives(d: dict[str, int]) -> dict[str, int]:
    return {k: v for k, v in d.items() if v > 0}
