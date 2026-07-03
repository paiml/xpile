# PMAT-1166: the identity-vs-structural divergence (PMAT-1165) reached
# TRANSITIVELY through a container. `Obj` is a PLAIN class (not @dataclass, no
# __eq__/__ne__), so Python compares its instances by object IDENTITY. A list
# `==` compares elements with `==`, so `[Obj(1)] == [Obj(1)]` is False in CPython
# (distinct objects) — but xpile derives a structural `PartialEq` and the emitted
# `Vec == Vec` compares field-wise, silently returning true. Refused fail-loud.
class Obj:
    def __init__(self, x: int) -> None:
        self.x = x


def compare() -> bool:
    a = [Obj(1)]
    b = [Obj(1)]
    return a == b
