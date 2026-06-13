def sum_positive(xs: list[int]) -> int:
    return sum(x for x in xs if x > 0)


def sum_even_squares(n: int) -> int:
    return sum(i * i for i in range(n) if i % 2 == 0)


def keep_positive(xs: list[int]) -> list[int]:
    return list(x for x in xs if x > 0)
