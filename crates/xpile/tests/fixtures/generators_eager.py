# PMAT-1071: EAGER generators — a `def g() -> T: … yield e …` function is
# rewritten (AST pre-transform, before the sig pre-pass) into a list-BUILDING
# function: `__gen_result: list[T] = []`, each `yield e` → `__gen_result.append
# (e)`, trailing `return __gen_result`, return type `T` → `list[T]`. So
# `for x in g()` / `list(g())` / `sum(g())` ride the existing list machinery.
# Covers straight-line + loop + conditional yields, stateful generators (fib),
# and early bare `return` (stop). The yield type comes from `-> T` or
# `-> Iterator[T]`/`Iterable[T]`/`Generator[T, …]`. First-cut limits (precise
# refusals): unannotated generators, bare `yield`, `yield from`, value-`return`.
# NOTE: eager = finite generators only (an unbounded generator would not
# terminate); the common finite `for x in g()` case is faithful.
# Differentially verified vs CPython (14/3/8/4).
def squares(n: int) -> int:
    for i in range(n):
        yield i * i


def evens_upto(n: int) -> int:
    for i in range(n):
        if i % 2 == 0:
            yield i


def fib(n: int) -> int:
    a: int = 0
    b: int = 1
    for i in range(n):
        yield a
        c: int = a + b
        a = b
        b = c


def upto(limit: int) -> int:
    for i in range(100):
        if i >= limit:
            return
        yield i


def sum_of_squares(n: int) -> int:
    return sum(squares(n))


def evens_list(n: int) -> int:
    return len(list(evens_upto(n)))


def fib_seventh() -> int:
    return list(fib(7))[6]


def early_stop_count() -> int:
    return len(list(upto(4)))

