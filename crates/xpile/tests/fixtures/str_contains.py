# PMAT-502o (Tranche 2): substring containment `sub in s` / `sub not in s`
# when the right operand is a str -> (s).contains(&(sub)[..]).
def has(s: str, sub: str) -> bool:
    return sub in s


def lacks(s: str, sub: str) -> bool:
    return sub not in s


def has_literal(s: str) -> bool:
    return "lo" in s
