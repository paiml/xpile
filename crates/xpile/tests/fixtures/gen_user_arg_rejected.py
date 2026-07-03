# PMAT-1093 (skeptic pass PMAT-1090, binding-class): passing a generator to
# USER code hands CPython an ITERATOR (the second loop in consume_twice sees
# an exhausted iterator: 3 + 0 = 3) but hands the eager lowering a plain
# list (3 + 3 = 6 — SILENT wrong answer). Only the fully-consuming builtins
# (sum/min/max/any/all/list/sorted/set) accept a generator argument.
def gen(n: int) -> int:
    for i in range(n):
        yield i


def consume_twice(xs: list[int]) -> int:
    a: int = 0
    for x in xs:
        a = a + x
    b: int = 0
    for x in xs:
        b = b + x
    return a + b


def entry() -> int:
    return consume_twice(gen(3))
