# PMAT-1026 (sweep #10 finding 2): free-function STRUCT returns — the factory
# idiom (`make`), a param-threading factory (`boost`, ctor-arg chain), and the
# struct result bound + mutated at the call site. Previously every one of
# these call sites refused "type mismatch — expected WASM i32 but expression
# lowered to i64" on the WASM lane (the PMAT-1023 AssocFnRegistry fixed only
# `Class::__init__` sites). Executes on BOTH lanes, value-matched vs CPython
# (4112).


class Counter:
    def __init__(self, n: int) -> None:
        self.n: int = n

    def incr(self) -> None:
        self.n = self.n + 1

    def get(self) -> int:
        return self.n


def make(start: int) -> Counter:
    return Counter(start)


def boost(c: Counter, k: int) -> Counter:
    c.incr()
    return Counter(c.get() + k)


def run() -> int:
    a = make(3)
    a.incr()
    c = boost(Counter(10), 100)
    c.incr()
    return a.get() * 1000 + c.get()
