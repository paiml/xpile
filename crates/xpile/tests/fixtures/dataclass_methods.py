from dataclasses import dataclass


@dataclass
class Rect:
    w: int
    h: int

    def area(self) -> int:
        # Read-only method: reads self fields.
        return self.w * self.h

    def scaled_area(self, k: int) -> int:
        # Method with an extra param; calls another method on self.
        return self.area() * k


def total(r: Rect) -> int:
    # Method call on a struct param.
    return r.area() + r.scaled_area(2)
