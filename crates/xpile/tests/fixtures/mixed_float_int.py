def at_least(x: float, n: int) -> bool:
    # mixed float/int comparison (Rust rejects f64 <op> i64).
    return x >= n


def is_whole_three(x: float) -> bool:
    return x == 3


def scaled(x: float) -> float:
    # mixed float/int arithmetic — int promoted to f64.
    return x * 2 + 1


def half_past(x: float) -> bool:
    # chained comparison with int bounds over a float.
    return 0 < x < 10


def int_times_float(x: float) -> float:
    return 3 * x
