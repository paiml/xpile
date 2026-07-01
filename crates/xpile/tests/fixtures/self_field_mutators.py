# PMAT-1022: self.FIELD container mutators. `self.items.append(x)` statements
# lower via MethodCall{push} (was refused with the subprocess message);
# expression-position `self.items.pop()` unwraps the field-read clone so the
# pop mutates the REAL field (was silently popping a copy); both drive
# &mut self via the extended AST detector (field-mutator receivers +
# value-position scanning). Scalar-returning methods (`take -> int`) do NOT
# flag as aliasing (return-type gate); container-returning methods
# (`get_items -> list[int]`) draw result~receiver edges (reject fixture).
class Bag:
    items: list[int]

    def __init__(self, items: list[int]) -> None:
        self.items = items

    def add(self, x: int) -> None:
        self.items.append(x)

    def take(self) -> int:
        return self.items.pop()

    def size(self) -> int:
        return len(self.items)


def run() -> int:
    b = Bag([1, 2])
    b.add(5)
    b.add(6)
    t = b.take()
    return b.size() * 100 + t


class P:
    x: int

    def __init__(self, x: int) -> None:
        self.x = x

    def me(self) -> "P":
        return self


def chain() -> int:
    p = P(3)
    q = p.me()
    return q.x
