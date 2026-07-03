# PMAT-1083 (skeptic pass PMAT-1081, probe p12-next): `next(g)` needs the
# iterator protocol — the eager model materializes a whole list and has no
# per-item cursor. Previously emitted a free `next(it)` call (rustc E0425,
# loud but far downstream). Must refuse precisely at lowering.
def gen(n: int) -> int:
    for i in range(n):
        yield i


def entry() -> int:
    it = gen(3)
    return next(it)
