# PMAT-1093 (skeptic pass PMAT-1090, B-F1-assign-position): a side-effect
# call in ASSIGN position (`v = noisy(i)`) evaded the PMAT-1083 statement-
# position scan — lazily it interleaves with consumption; eagerly it all runs
# at materialization time (side,side,1,2 vs CPython side,1,side,2). Any call
# in a generator body now refuses unless the callee is a pure builtin
# (range/len/abs) or another generator.
def noisy(x: int) -> int:
    print("side")
    return x


def gen(n: int) -> int:
    for i in range(n):
        v = noisy(i)
        yield v


def entry() -> int:
    return sum(gen(3))
