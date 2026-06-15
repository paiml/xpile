# PMAT-627: a default-using function called in argument position of another call.
# The outer call was default-filled, but the nested call (lowered context-free by
# lower_call) was left under-applied → E0061. Default-filling now recurses into
# nested user-calls in argument position.
def inc(x: int, by: int = 1) -> int:
    return x + by


def into_user(x: int) -> int:
    return inc(inc(x))


def deep(x: int) -> int:
    return inc(inc(inc(x)))


def two_args(x: int, y: int) -> int:
    return inc(inc(x), inc(y))
