# PMAT-1049 (sweep #12): a subscript store whose INDEX borrows the base —
# `self.xs[self.next_slot()] = v` (next_slot() needs &mut self) or
# `self.xs[self.xs.pop()] = v` (pop needs &mut self.xs) — emitted the index
# INSIDE the `&mut base` borrow → rustc E0499. The write-through emitter now
# binds every step index to a temp BEFORE `&mut base`. Verified vs CPython.
class Ring:
    xs: list[int]
    pos: int

    def __init__(self) -> None:
        self.xs = [0, 0, 0]
        self.pos = 0

    def next_slot(self) -> int:
        self.pos = (self.pos + 1) % 3
        return self.pos

    def push(self, v: int) -> None:
        self.xs[self.next_slot()] = v


def ring_result() -> int:
    r: Ring = Ring()
    r.push(5)
    r.push(6)
    return r.xs[0] * 100 + r.xs[1] * 10 + r.xs[2] + r.pos
