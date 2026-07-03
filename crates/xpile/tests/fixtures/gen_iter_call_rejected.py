# PMAT-1093 (skeptic pass PMAT-1090, B-F3-iter-position): a side-effect call
# in the for-loop ITER position (`for x in src()`) evaded the PMAT-1083
# statement-position scan — `src()` runs at materialization time eagerly vs
# first-next time lazily. Pure `range(...)` and calls to other generators
# stay accepted in this slot.
def src() -> list[int]:
    print("side")
    return [1, 2, 3]


def gen() -> int:
    for x in src():
        yield x


def entry() -> int:
    return sum(gen())
