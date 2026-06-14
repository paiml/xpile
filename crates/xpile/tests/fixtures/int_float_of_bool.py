def bool_to_int(b: bool) -> int:
    # int(True) == 1, int(False) == 0.
    return int(b)


def count_true(bs: list[bool]) -> int:
    # the canonical "count of True" idiom: sum of int(b).
    return sum(int(b) for b in bs)


def predicate_to_int(x: int) -> int:
    # int() of a comparison result.
    return int(x > 0) + int(x > 10)


def bool_to_float_scaled(b: bool) -> float:
    # float(True) == 1.0; float(bool) must cast through i64 in Rust.
    return float(b) * 2.5
