# PMAT-502cg (Tranche 2): list & set comprehensions over `d.items()` with a
# tuple target — completing the comprehension-over-items family (dict-comp was
# PMAT-502cf). Both desugar to a ForEachPair(Pairs) loop; `if` filter composes.
def values(d: dict[str, int]) -> list[int]:
    return [v for k, v in d.items()]


def pos_keys(d: dict[str, int]) -> list[str]:
    return [k for k, v in d.items() if v > 0]


def value_set(d: dict[str, int]) -> set[int]:
    return {v for k, v in d.items()}
