# PMAT-1024: Python OOP ALIASING — the exact shape the Rust lane must REFUSE
# (value semantics cannot express object sharing; PMAT-1016C/1020) executes
# natively on the WASM heap: every binding of a record holds the SAME i32
# base-pointer, so a mutation through one name is visible through every alias,
# exactly like CPython.
#
# CPython: a is b -> three incr() calls hit ONE object -> 3 + 3 = 6.
# (A per-binding clone would print 1 + 2 = 3 — the silent divergence the
# Rust lane's refusal exists to prevent.)


class Counter:
    count: int

    def __init__(self, count: int) -> None:
        self.count = count

    def incr(self) -> None:
        self.count = self.count + 1

    def get(self) -> int:
        return self.count


def run() -> int:
    a = Counter(0)
    b = a
    b.incr()
    b.incr()
    a.incr()
    return a.get() + b.get()
