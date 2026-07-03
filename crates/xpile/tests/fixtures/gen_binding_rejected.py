# PMAT-1093 (skeptic pass PMAT-1090, B-F7-double-consumption): a BOUND
# generator has iterator IDENTITY — CPython's second sum(g) sees an EXHAUSTED
# iterator (14 then 0, total 14); the eager list has no cursor (14 then 14,
# total 28 — SILENT wrong answer). Bindings refuse; consume at the call site
# or materialize honestly with list(...).
def gen(n: int) -> int:
    for i in range(n):
        yield i * i


def entry() -> int:
    g = gen(4)
    return sum(g) + sum(g)
