# PMAT-1085 (finding b): a loop-var reassignment buried in a try/except body
# needs the PMAT-1080 `for mut x` gating too — the scan's catch-all missed
# Stmt::TryCatch, so this was rustc E0594 (loud). CPython: 60.
def try_scale() -> int:
    total: int = 0
    for x in [1, 2, 3]:
        try:
            x = x * 10
        except ValueError:
            pass
        total = total + x
    return total


# Precision guards for the PMAT-1085 same-name-nesting refusal: SIBLING
# same-name loops are one-scope-at-a-time (supported), and nested `_` loops
# are the common count-only idiom (`_` is exempt — it cannot be read).
# CPython: (1+2) + (3+4) + 4 = 14.
def siblings_ok() -> int:
    total: int = 0
    for x in [1, 2]:
        total = total + x
    for x in [3, 4]:
        total = total + x
    for _ in [1, 2]:
        for _ in [1, 2]:
            total = total + 1
    return total
