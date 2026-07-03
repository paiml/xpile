# PMAT-1095 (skeptic pass PMAT-1090, C-F4): a FRESH binding inside a loop
# body re-`let`s per iteration — no prior binding is mutated, so it must
# NOT emit `let mut` (the user-facing `clippy -D warnings` gate rejects
# unused_mut; the pre-walk previously doubled EVERY in-loop store). Only a
# genuine same-iteration reassignment stays `mut`; a pre-loop binding
# stored in the body and a body local read after the loop (PMAT-1038
# hoist) keep their `mut` via other paths.
def fresh_scalar(xs: list[int]) -> int:
    total: int = 0
    for i in xs:
        a = i + 1
        total = total + a
    return total


def fresh_tuple(xs: list[int]) -> int:
    total: int = 0
    for i in xs:
        a, b = i + 1, 5
        total = total + a + b
    return total


def fresh_branch(xs: list[int]) -> int:
    total: int = 0
    for i in xs:
        if i > 1:
            a = 10
        else:
            a = 20
        total = total + a
    return total


def reassigned_still_mut(xs: list[int]) -> int:
    total: int = 0
    for i in xs:
        a = i + 1
        a = a * 2
        total = total + a
    return total


# PMAT-1095 (adjacent, found first-hand): an ANNOTATED store to the
# PMAT-1038-hoisted loop local is a REASSIGNMENT of the live binding —
# the annotated path instead emitted a shadowing `let`, so the hoisted
# `let mut a = 0` never saw the body's stores and this returned the
# default 0 instead of CPython's leaked last value (3).
def annotated_leak(xs: list[int]) -> int:
    for i in xs:
        a: int = i + 1
    return a
