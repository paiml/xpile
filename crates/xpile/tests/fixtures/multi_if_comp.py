def genexpr_2if(xs: list[int]) -> int:
    # Two `if` filters in a generator expression are ANDed.
    return sum(x for x in xs if x > 0 if x < 100)


def listcomp_2if(xs: list[int]) -> int:
    ys = [x for x in xs if x > 0 if x % 2 == 0]
    return len(ys)


def listcomp_range_2if(n: int) -> int:
    ys = [i for i in range(n) if i > 2 if i < 8]
    return len(ys)


def setcomp_2if(xs: list[int]) -> int:
    s = {x % 10 for x in xs if x > 0 if x < 50}
    return len(s)


def dictcomp_2if(xs: list[int]) -> int:
    d = {x: x * x for x in xs if x > 0 if x < 10}
    return len(d)


def three_if(xs: list[int]) -> int:
    # Three filters fold to a left-nested `a && b && c`.
    return sum(x for x in xs if x > 0 if x < 100 if x % 3 == 0)
