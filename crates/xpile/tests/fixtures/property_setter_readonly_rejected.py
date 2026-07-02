# PMAT-1056: assigning a read-only `@property` (no `@c.setter`) must REFUSE —
# Python raises AttributeError ("can't set attribute"). xpile must not silently
# create a phantom field; it names the read-only property clearly.
class Temp:
    def __init__(self, c: float) -> None:
        self._c = c

    @property
    def c(self) -> float:
        return self._c


def bad(t: Temp) -> float:
    t.c = 5.0
    return t.c
