def sh(x: int, n: int) -> int:
    # `x << n` must PANIC on i64 value overflow (C-PY-INT-ARITH promises a
    # panic until bigint promotion lands) — `checked_shl` alone only guards the
    # shift *amount*, so `1 << 63` previously wrapped to i64::MIN silently.
    return x << n


def small() -> int:
    return 1 << 10


def fits_exactly() -> int:
    # -2 << 62 == i64::MIN — fits exactly, must NOT panic.
    return -2 << 62
