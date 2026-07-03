# PMAT-1083 (skeptic pass PMAT-1081, probe p1-interleave): a side-effect call
# statement between yields is lazily INTERLEAVED with consumption (CPython
# prints side,1,side2,2) but the eager list-materializing lowering runs the
# whole body before the first item is consumed (side,side2,1,2) — a SILENT
# reordering. Must refuse at the generator transform.
def noisy(n: int) -> int:
    for i in range(n):
        print("side")
        yield i


def entry() -> int:
    return sum(noisy(3))
