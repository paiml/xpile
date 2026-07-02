# PMAT-1027 (sweep #10 finding 4): struct methods that happen to share
# builtin-container-mutator NAMES (`add`, `pop`, `update`), mutated through an
# alias. The name-keyed `collect_container_mutated` classified these as
# container-shaped and false-refused the whole program on the reference lane
# (`b.add(5)` refused while an identical `b.plus(5)` executed). The typed
# classifier proves `b`'s alias class is unanimously `Bag` and executes all
# three; the Rust lane keeps refusing (genuine mutation through an alias is
# unrepresentable under value semantics). CPython ground truth: 1614.


class Bag:
    def __init__(self, n: int) -> None:
        self.n: int = n

    def add(self, v: int) -> None:
        self.n = self.n + v

    def pop(self) -> int:
        self.n = self.n - 1
        return self.n

    def update(self, v: int) -> None:
        self.n = self.n + v


def run() -> int:
    a = Bag(10)
    b = a
    b.add(5)
    x: int = b.pop()
    b.update(2)
    return a.n * 100 + x
