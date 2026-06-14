def ic(x: float) -> int:
    # Python returns an exact arbitrary-precision int for an out-of-i64-range
    # finite float (int(1e30) == 10**30-ish), but Rust's `as i64` saturates to
    # i64::MAX silently. xpile can't represent the bignum yet, so it fails loud
    # (panic) rather than return the wrong value. In-range floats truncate
    # toward zero as before.
    return int(x)
