# PMAT-643: str.isnumeric() — True iff non-empty and every char is numeric
# (Unicode Number categories Nd/Nl/No), broader than isdigit (e.g. ½, ² are
# numeric). Completes the str-classification-predicate family.
def is_num(s: str) -> int:
    return 1 if s.isnumeric() else 0


# isdigit stays distinct (a fraction is numeric but not a digit) — regression.
def is_dig(s: str) -> int:
    return 1 if s.isdigit() else 0
