def fsum(xs: list[float]) -> float:
    # CPython 3.12+ sum() over floats uses Neumaier compensated summation;
    # naive left-to-right would give 0.0 here (catastrophic cancellation).
    return sum(xs)


def fsum_ones() -> float:
    # naive sum([0.1]*10) == 0.9999999999999999; compensated == 1.0.
    xs: list[float] = [0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1]
    return sum(xs)


def fsum_start(xs: list[float], s: float) -> float:
    return sum(xs, s)


def isum(xs: list[int]) -> int:
    # int sum stays exact.
    return sum(xs)
