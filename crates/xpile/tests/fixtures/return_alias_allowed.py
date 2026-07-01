# PMAT-1018 companion: the shapes that stay ACCEPTED next to the
# laundered-alias refusal — a fn constructing a FRESH struct (not
# param-returning), a laundered alias whose arg is never re-read (move),
# and a laundered alias with no mutation anywhere (clone-identical).
class Counter:
    n: int

    def __init__(self, n: int) -> None:
        self.n = n

    def bump(self) -> None:
        self.n = self.n + 1

    def get(self) -> int:
        return self.n


def fresh(c: Counter) -> Counter:
    return Counter(c.get() + 100)


def ident(c: Counter) -> Counter:
    return c


def fresh_construct(n: int) -> int:
    c = Counter(n)
    d = fresh(c)
    d.bump()
    return c.get() + d.get()


def moved_launder(n: int) -> int:
    c = Counter(n)
    d = ident(c)
    d.bump()
    return d.get()


def readonly_launder(n: int) -> int:
    c = Counter(n)
    d = ident(c)
    return d.get() + c.get()
