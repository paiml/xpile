# PMAT-1022 rejects: (c) a method RETURNING its container field aliases the
# receiver — the caller's mutation landed on a detached clone (SILENT 2≠3);
# now the return-type-gated result~receiver edge refuses. (d) passing a
# struct's list FIELD to a param-mutating helper cloned the field (SILENT);
# the PMAT-884 guard now matches FieldAccess args.
class Bag:
    items: list[int]

    def __init__(self, items: list[int]) -> None:
        self.items = items

    def get_items(self) -> list[int]:
        return self.items


def main() -> int:
    b = Bag([1, 2])
    xs = b.get_items()
    xs.append(9)
    return len(b.get_items())
