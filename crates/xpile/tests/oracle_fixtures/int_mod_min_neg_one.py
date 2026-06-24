def main() -> None:
    # HUNT-V17 ND-1 (PMAT-929): `i64::MIN % -1` is `0` in Python, NOT an overflow.
    # The old codegen used `checked_rem(..).expect("modulo overflow")`, which
    # panicked here (the only i64 input besides a zero divisor where `checked_rem`
    # is `None`), diverging from CPython. Any integer is exactly divisible by ±1,
    # so the true remainder is `0`; `wrapping_rem` recovers it. (Floor-DIV of the
    # same operands genuinely overflows to 2**63, so it is intentionally NOT here.)
    big_min: int = -9223372036854775808
    minus_one: int = -1
    print(big_min % minus_one)
    print(big_min % 1)
    # Regression: ordinary sign-aware floor-modulo stays correct.
    print(7 % 3)
    print(-7 % 3)
    print(7 % -3)
    print(-7 % -3)
    print(5 % -1)
    print(0 % -1)
    print(100 % 7)
