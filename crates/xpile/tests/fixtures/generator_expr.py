def sum_squares(n: int) -> int:
    return sum(i * i for i in range(n))


def sum_abs(xs: list[int]) -> int:
    return sum(abs(x) for x in xs)


def max_abs(xs: list[int]) -> int:
    return max(abs(x) for x in xs)


def doubled(xs: list[int]) -> list[int]:
    return list(x * 2 for x in xs)
