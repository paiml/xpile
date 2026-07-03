# PMAT-1093 (skeptic pass PMAT-1090, B-F5-hang-class): a `break` out of a
# `for` over a generator call stops the generator EARLY in CPython (lazy —
# instant even over range(10**11)); the eager lowering has already
# materialized the ENTIRE sequence before the loop starts (a hang) — the
# same class the `while` refusal closed, reproduced through a huge `range`.
# Partial consumption refuses; consume fully or list(...) first.
def squares(n: int) -> int:
    for i in range(n):
        yield i * i


def entry() -> int:
    total: int = 0
    for v in squares(100000000000):
        if v > 10:
            break
        total = total + v
    return total
