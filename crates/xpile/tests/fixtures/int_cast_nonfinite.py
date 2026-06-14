def ic(x: float) -> int:
    # Python raises OverflowError for int(inf) and ValueError for int(nan);
    # Rust's `as i64` saturates (inf -> i64::MAX) / zeroes (nan -> 0) silently.
    # int(finite) truncates toward zero (matching Python).
    return int(x)


def ii(n: int) -> int:
    # int(int) is identity — no guard, no clone.
    return int(n)
