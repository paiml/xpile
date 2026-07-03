# PMAT-1093 (skeptic pass PMAT-1090, B-F2-yield-position): a side-effect
# call inside the YIELD expression (`yield noisy(i)`) evaded the PMAT-1083
# statement-position scan — same eager-vs-lazy reordering as the assign case.
def noisy(x: int) -> int:
    print("side")
    return x


def gen(n: int) -> int:
    for i in range(n):
        yield noisy(i)


def entry() -> int:
    return sum(gen(3))
