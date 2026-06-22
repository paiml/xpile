def add3(a: int, b: int, c: int) -> int:
    return a + b + c


def add2(a: int, b: int) -> int:
    return a + b


def double(x: int) -> int:
    return x * 2


def nested_in_user_fn(xs: list[int]) -> int:
    # `g(*xs)` splat nested as an arg to a USER-defined outer fn (was rejected
    # with a misleading "missing argument" before PMAT-877).
    return double(add3(*xs))


def deep_nest(xs: list[int]) -> int:
    return double(double(add3(*xs)))


def sibling_splats(xs: list[int], ys: list[int]) -> int:
    # two splats as sibling args of one outer call.
    return add2(add3(*xs), add2(*ys))


def nested_then_reuse(xs: list[int]) -> int:
    # nested splat + a second splat of the same list; list stays usable after.
    return double(add3(*xs)) + add3(*xs) + len(xs)
