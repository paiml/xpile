# PMAT-1016C: struct alias `c2 = c` — three-way disposition. Python shares
# the object; value semantics cannot. (1) NEITHER name mutated → CLONE
# (observably identical; the read-only alias was E0382). (2) source never
# re-read → MOVE. (3) mutation with both names live → clean REFUSAL (the
# reject fixture below). Function-wide conservative via ctx.mutable +
# read_counts.
class Counter:
    count: int

    def __init__(self, start: int) -> None:
        self.count = start

    def bump(self) -> None:
        self.count = self.count + 1

    def value(self) -> int:
        return self.count


def read_only_alias(n: int) -> int:
    c = Counter(n)
    c2 = c
    return c2.value() + c.value()


def move_alias(n: int) -> int:
    c = Counter(n)
    c2 = c
    c2.bump()
    return c2.value()
