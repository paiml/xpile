# PMAT-1025 (sweep #10): field DISCOVERY from `__init__` — no class-body
# declarations at all. `self.n: int = n` is the PEP 526 idiom typed Python
# actually writes; `self.hits = 0` infers from the literal; `self.limit =
# limit` infers from the parameter's annotation. Previously every one of
# these REFUSED ("non-Name annotated-assignment target" / "no such field") —
# the class-body-declaration style was the only accepted shape, and the
# shipped OOP fixtures quietly dodged the standard idiom.


class Tally:
    def __init__(self, n: int, limit: int) -> None:
        self.n: int = n
        self.limit = limit
        self.hits = 0

    def incr(self) -> None:
        self.n = self.n + 1
        self.hits = self.hits + 1

    def full(self) -> bool:
        return self.n >= self.limit

    def get(self) -> int:
        return self.n


def run() -> int:
    t = Tally(3, 5)
    t.incr()
    t.incr()
    total = t.get() * 100 + t.hits * 10
    if t.full():
        total = total + 1
    return total
