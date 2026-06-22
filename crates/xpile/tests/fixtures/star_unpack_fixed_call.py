def add3(a: int, b: int, c: int) -> int:
    return a + b + c


def join2(x: str, y: str) -> str:
    return x + "-" + y


def take_one(a: int) -> int:
    return a * 10


def sum_pair(xs: list[int]) -> int:
    return add3(*xs)


def call_add3(a: int, b: int, c: int) -> int:
    nums = [a, b, c]
    return add3(*nums)


def call_join2() -> str:
    parts = ["left", "right"]
    return join2(*parts)


def call_take_one() -> int:
    single = [7]
    return take_one(*single)


def splat_then_reuse(xs: list[int]) -> int:
    # PMAT-876 move-bug regression: the list must stay usable after the splat
    # (Python iterates `*xs` without consuming it). A by-value bind would E0382.
    first = add3(*xs)
    return first + len(xs) + xs[0]


def splat_twice(xs: list[int]) -> int:
    # Splatting the SAME variable into two calls — both borrows, no move.
    return add3(*xs) + add3(*xs)


def splat_literal() -> int:
    # A LITERAL source is a fresh value → temp-bound (the `let __xpile_splat`
    # path); there is no variable to keep alive, so moving the temp is safe.
    return add3(*[1, 2, 3])
