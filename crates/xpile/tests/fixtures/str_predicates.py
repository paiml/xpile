# PMAT-502ag (Tranche 2): string classification predicates
# .isdigit()/.isalpha()/.isspace() -> Bool. Empty string is False (Python).
def all_digits(s: str) -> bool:
    return s.isdigit()


def all_alpha(s: str) -> bool:
    return s.isalpha()


def all_space(s: str) -> bool:
    return s.isspace()
