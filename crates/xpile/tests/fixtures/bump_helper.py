# PMAT-1024: the MUTATING-HELPER idiom — a function that mutates its object
# parameter in place. The Rust lane REFUSES this (PMAT-884: the ownership
# clone would silently drop the mutation); the WASM lane passes the record's
# i32 base-pointer, so bump() mutates the CALLER's object exactly like
# CPython's pass-by-reference.
#
# CPython: two bump() calls on one object -> 2.
class Counter:
    count: int

    def __init__(self, count: int) -> None:
        self.count = count

    def incr(self) -> None:
        self.count = self.count + 1

    def get(self) -> int:
        return self.count


def bump(c: Counter) -> None:
    c.incr()


def run() -> int:
    c = Counter(0)
    bump(c)
    bump(c)
    return c.get()
