# PMAT-502cn (Tranche 2): 2-arg min/max over str/bool (lexicographic / Ord).
# Previously min/max only accepted int/float operands (str fell through to an
# undefined Rust `min(...)`).
def smaller(a: str, b: str) -> str:
    return min(a, b)


def larger(a: str, b: str) -> str:
    return max(a, b)
