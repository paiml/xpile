# PMAT-1016C case (3): alias + mutation with BOTH names observable — Python's
# shared mutation (c2.bump() visible through c) is unrepresentable: the old
# emit was rustc E0382 (fail-loud but invalid emit); a clone would SILENTLY
# print 0 where Python prints 1. Clean-refused at lowering.
class Counter:
    count: int

    def __init__(self, start: int) -> None:
        self.count = start

    def bump(self) -> None:
        self.count = self.count + 1

    def value(self) -> int:
        return self.count


def main() -> int:
    c = Counter(0)
    c2 = c
    c2.bump()
    return c.value()
