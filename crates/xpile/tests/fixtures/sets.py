# PMAT-500 (Tranche 2): sets — literal {..} + `x in s` / `x not in s`.
# {a, b, c} -> HashSet-init block; x in s -> s.contains(&(x)). Lean refuses.
def is_vowel(c: str) -> bool:
    return c in {"a", "e", "i", "o", "u"}


def is_small(x: int) -> bool:
    return x in {1, 2, 3}


def not_member(x: int) -> bool:
    return x not in {10, 20, 30}
