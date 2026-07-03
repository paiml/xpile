# PMAT-1165: a PLAIN class (not @dataclass, defining neither __eq__ nor __ne__)
# uses Python's DEFAULT equality, which is object IDENTITY — two distinct
# instances are NEVER `==` (`Obj(5) == Obj(5)` is False in CPython). xpile
# derives a structural `PartialEq` for the struct, so the emitted `a == b` would
# silently return `true` where CPython returns `False`. xpile's value/clone model
# cannot express object identity, so `==`/`!=` between two such instances is
# refused fail-loud (SKIP-EMIT) rather than transpiled to the silently-wrong
# structural comparison. Define `__eq__` or use `@dataclass` for structural `==`.
class Obj:
    def __init__(self, x: int) -> None:
        self.x = x


def compare() -> bool:
    a = Obj(5)
    b = Obj(5)
    return a == b
