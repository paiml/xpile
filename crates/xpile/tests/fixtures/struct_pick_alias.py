# PMAT-1026 (sweep #10 finding 2): the returns-param IDENTITY shape through a
# free function. `pick` returns its argument, so `b = pick(a)` makes `b` the
# SAME object as `a` — CPython reference semantics. The Rust lane REFUSES
# (the PMAT-1020 alias-class analysis: value semantics cannot express the
# sharing); the WASM lane EXECUTES it exactly (the free-fn call returns the
# record's i32 base-pointer, previously mistyped as a conservative i64).


class Counter:
    def __init__(self, n: int) -> None:
        self.n: int = n

    def incr(self) -> None:
        self.n = self.n + 1


def pick(c: Counter) -> Counter:
    return c


def run() -> int:
    a = Counter(10)
    b = pick(a)
    b.incr()
    b.incr()
    return a.n
