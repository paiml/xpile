class Counter:
    count: int

    def __init__(self, count: int) -> None:
        self.count = count

    def incr(self) -> None:
        self.count = self.count + 1

    def get(self) -> int:
        return self.count


def run() -> int:
    c = Counter(0)
    c.incr()
    c.incr()
    return c.get()
