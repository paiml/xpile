# PMAT-470 (R1): augmented assignment x <op>= e, the most-used Python
# loop idiom. Desugars to x = x <op> e — no meta-HIR / backend change.
def count_up(n: int) -> int:
    total = 0
    i = 0
    while i < n:
        total += i
        i += 1
    return total


def product(xs: list[int]) -> int:
    p = 1
    for x in xs:
        p *= x
    return p


def shout(s: str) -> str:
    out = s
    out += "!"
    out += "!"
    return out
