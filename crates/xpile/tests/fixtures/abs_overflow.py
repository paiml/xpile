def a_int(x: int) -> int:
    # `abs` of an i64 must be overflow-checked: `i64::MIN.abs()` wraps to
    # i64::MIN silently (no panic under `-O`), falsifying C-PY-INT-ARITH
    # (Python's abs is exact). Now emits `.checked_abs().expect(...)`.
    return abs(x)


def a_float(x: float) -> float:
    # f64 abs never overflows — keeps `.abs()`.
    return abs(x)
