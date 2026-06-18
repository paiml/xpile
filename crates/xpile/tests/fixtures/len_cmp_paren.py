# PMAT-761 (HUNT-V16 CFD-3): `len(x) < N` emitted `x.len() as i64 < N`, and
# rustc reads `i64 <` as the start of generic arguments (a turbofish) → a hard
# parse error. Only `<` triggered it (`<=`/`>`/`==` parsed fine). Parenthesizing
# the cast — `(x.len() as i64)` — disambiguates it everywhere. Cross-checked vs
# python3.


def count_while_small(xs: list[int]) -> int:
    total = 0
    for x in xs:
        if len(xs) < 6:
            total = total + 1
    return total


def while_len_lt(n: int) -> int:
    xs: list[int] = []
    while len(xs) < n:
        xs.append(0)
    return len(xs)


def len_le_and_gt(xs: list[int]) -> int:
    # the already-working comparison operators must still work
    a = 1 if len(xs) <= 3 else 0
    b = 1 if len(xs) > 1 else 0
    return a * 10 + b
