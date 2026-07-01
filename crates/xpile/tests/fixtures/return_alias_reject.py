# PMAT-1018: RETURN-aliasing — `ident` returns its parameter, so in Python
# `d = ident(c)` makes d and c the SAME object; xpile's pass-by-value (the
# PMAT-588 clone at the call) made d a COPY, so `d.bump()` was invisible
# through c: rust 30 vs cpython 31 — the one SILENT finding of sweep #8.
# The FnSig now carries `returns_param` (a return-position bare param Name,
# recursing through if/while/for), and binding a struct result of such a fn
# applies the PMAT-1016C three-way test to (arg, binding): refused here
# (mutation + both names live).
class Counter:
    n: int

    def __init__(self, n: int) -> None:
        self.n = n

    def bump(self) -> None:
        self.n = self.n + 1

    def get(self) -> int:
        return self.n


def ident(c: Counter) -> Counter:
    return c


def main() -> int:
    c = Counter(30)
    d = ident(c)
    d.bump()
    return c.get()
