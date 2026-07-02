# PMAT-1037 guard: a field-subscript STORE through one alias of a struct
# while the other alias is still observed — Python shares the object (c
# sees 99); Rust's value model would clone. Must REFUSE, not miscompile
# (collect_obj_mutated counts `c2.counts[0] = v` as object mutation via
# subscript_chain_attr_base).
class C:
    counts: list[int]

    def __init__(self) -> None:
        self.counts = [1, 2]


def main() -> None:
    c = C()
    c2 = c
    c2.counts[0] = 99
    print(c.counts[0])
