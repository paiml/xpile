# PMAT-1056: `self.<prop> = v` inside __init__ where <prop> is a settable
# @property is REFUSED (honest deferral) — the synthesized constructor builds and
# returns the struct value, so it cannot call a setter method on a not-yet-built
# `self`. The user should write the backing field directly.
class Temp:
    def __init__(self, c: float) -> None:
        self.c = c

    @property
    def c(self) -> float:
        return self._c

    @c.setter
    def c(self, v: float) -> None:
        self._c = v
