# PMAT-1056: `@<prop>.setter` support. The getter `@property def c` lowers to a
# `&self` method `c()`; its writable partner `@c.setter def c(self, v)` lowers to
# a `&mut self` method RENAMED `set_c` (no collision with the getter), and an
# assignment `obj.c = v` is rewritten to `obj.set_c(v)`. `self.c = v` inside a
# non-__init__ method routes through the setter too. Differentially checked vs
# CPython in oracle_fixtures/property_setter.py.
class Temp:
    def __init__(self, c: float) -> None:
        self._c = c

    @property
    def c(self) -> float:
        return self._c

    @c.setter
    def c(self, v: float) -> None:
        self._c = v

    def reset(self) -> None:
        self.c = 0.0


def set_external(t: Temp, v: float) -> float:
    # external mutation of a built instance through its setter
    t.c = v
    return t.c


def set_from_int(t: Temp) -> float:
    # an int assigned to a float property widens like a field store
    t.c = 7
    return t.c


def reset_it(t: Temp) -> float:
    t.reset()
    return t.c
