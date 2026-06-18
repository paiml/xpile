# PMAT-781 (HUNT-V17 #10 IFM-1): an int argument passed to a float parameter
# (`scale(2, 3)` over `factor: float`) emitted a bare `2i64` against the Rust
# `f64` slot → rustc E0308. Python implicitly widens int→float at the call. The
# call-arg coercion now casts an int/bool arg to f64 when the param type is
# float; a float arg and an int param are unchanged. Cross-checked vs python3.


def scale(factor: float, n: int) -> float:
    return factor * n


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def use_int_arg() -> float:
    return scale(2, 3)


def use_all_int() -> float:
    return lerp(0, 10, 1)


def use_float_arg() -> float:
    return scale(2.5, 4)
