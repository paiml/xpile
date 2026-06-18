# PMAT-784 (HUNT-V17 #13 CR-01): Python LEAKS the loop variable — after `for x
# in xs:`, x holds the last element (or its pre-loop value if xs is empty).
# Rust's `for x in iter` scoped x to the loop, so a post-loop read saw the
# pre-loop value (silent-wrong: returned 0/`?`, not the leaked last elem). When
# the target is already bound with the element's type, the loop binding is now
# renamed to a fresh temp and `x = temp` assigned at the body top, so the loop
# updates the outer (let-mut) x. Cross-checked vs python3.


def last_seen(xs: list[int]) -> int:
    x = 0
    for x in xs:
        pass
    return x


def last_char(s: str) -> str:
    c = "?"
    for c in s:
        pass
    return c


def empty_keeps_prior(xs: list[int]) -> int:
    x = -1
    for x in xs:
        pass
    return x


def body_uses_var(xs: list[int]) -> int:
    total = 0
    x = 0
    for x in xs:
        total = total + x
    return total + x


def fresh_var_unaffected(xs: list[int]) -> int:
    total = 0
    for y in xs:
        total = total + y
    return total
