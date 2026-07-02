# PMAT-1056: `@<prop>.setter` — Python's writable-property idiom. A `@property`
# getter's writable partner is a second same-named method decorated
# `@<prop>.setter`; `obj.<prop> = v` calls it. xpile lowers the setter to a
# normal `&mut self` method RENAMED `set_<prop>` (so it does not collide with
# the getter `<prop>()`) and rewrites the assignment `obj.<prop> = v` to
# `obj.set_<prop>(v)`. The setter's validation logic (clamping here) is the whole
# point of a settable property. vs python3.
class Volume:
    def __init__(self, level: int) -> None:
        self._level = level

    @property
    def level(self) -> int:
        return self._level

    @level.setter
    def level(self, v: int) -> None:
        # A setter's raison d'être: enforce an invariant on assignment.
        if v < 0:
            self._level = 0
        elif v > 100:
            self._level = 100
        else:
            self._level = v

    def mute(self) -> None:
        # `self.<prop> = v` inside a NON-__init__ method routes to `self.set_level`.
        self.level = 0


def main() -> None:
    vol: Volume = Volume(50)
    print(vol.level)  # 50 (getter)
    vol.level = 150  # setter clamps high -> 100
    print(vol.level)  # 100
    vol.level = -20  # setter clamps low -> 0
    print(vol.level)  # 0
    vol.level = 73  # in range
    print(vol.level)  # 73
    vol.mute()  # self.level = 0 inside a method
    print(vol.level)  # 0
    # int literal assigned to an int property, then read back through the getter.
    vol.level = 42
    print(vol.level + 1)  # 43
