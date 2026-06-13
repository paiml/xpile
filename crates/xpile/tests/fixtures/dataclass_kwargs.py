from dataclasses import dataclass


@dataclass
class Point:
    x: int
    y: int


def all_kw() -> int:
    # All-keyword construction.
    p = Point(x=1, y=2)
    return p.x + p.y


def mixed() -> int:
    # Positional then keyword.
    p = Point(10, y=20)
    return p.x + p.y


def reordered() -> int:
    # Keywords out of declaration order — emitted in field order.
    p = Point(y=5, x=3)
    return p.x - p.y
