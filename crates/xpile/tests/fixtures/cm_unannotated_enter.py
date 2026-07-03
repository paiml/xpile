# PMAT-1103: the idiomatic UNANNOTATED context-manager `__enter__` — a class
# with `def __enter__(self): return self` (no return annotation) used in a
# `with cm as x:`. The with-desugar's `x = __cm.__enter__()` mistyped x as Unit
# (unannotated methods registered Type::Unit) while the impl emitted `-> Class`
# → rustc E0308 / false "non-struct" refusals. Fix: register an unannotated
# `__enter__` that `return self` with the class Struct type (matching the impl).
class Gate:
    tag: int

    def __init__(self, tag: int) -> None:
        self.tag = tag

    def __enter__(self):
        return self

    def __exit__(self, a: int, b: int, c: int) -> None:
        pass

    def doubled(self) -> int:
        return self.tag * 2


def use_method() -> int:
    result: int = 0
    with Gate(21) as x:
        result = x.doubled()
    return result


def use_field() -> int:
    result: int = 0
    with Gate(7) as x:
        result = x.tag
    return result
