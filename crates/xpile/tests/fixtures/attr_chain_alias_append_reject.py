# PMAT-1037 slice D guard (the d5 witness): an attribute-chain APPEND through
# one alias while the other stays observed — Python shares (len(b.items)
# sees 2); the value model's clone would silently drop it (rust 1). Before
# slice D the append itself refused at lowering, MASKING this gap in
# collect_obj_mutated's expr_mutator_receiver (Name-only receivers); the
# chain arm now counts `b2.items.append(9)` as object mutation of `b2`.
class B:
    items: list[int]

    def __init__(self) -> None:
        self.items = [1]


def main() -> None:
    b = B()
    b2 = b
    b2.items.append(9)
    print(len(b.items))
