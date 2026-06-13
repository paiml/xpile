from dataclasses import dataclass


@dataclass
class Rect:
    w: int
    h: int

    @property
    def area(self) -> int:
        return self.w * self.h

    @property
    def perimeter(self) -> int:
        return 2 * (self.w + self.h)

    def describe(self) -> int:
        # A method may read a property of self.
        return self.area + self.perimeter


def area_of(w: int, h: int) -> int:
    r = Rect(w, h)
    return r.area


def perimeter_of(w: int, h: int) -> int:
    r = Rect(w, h)
    return r.perimeter


def described(w: int, h: int) -> int:
    return Rect(w, h).describe()
