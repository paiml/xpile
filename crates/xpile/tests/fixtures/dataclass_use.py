from dataclasses import dataclass


@dataclass
class Point:
    x: int
    y: int


@dataclass
class Labeled:
    name: str
    value: int


def make(a: int, b: int) -> Point:
    # Positional construction → struct literal.
    return Point(a, b)


def dist_sq(p: Point) -> int:
    # Struct-typed param + field reads.
    return p.x * p.x + p.y * p.y


def origin_sum() -> int:
    # Struct local, then read its fields.
    o = Point(3, 4)
    return o.x + o.y


def label_len(lbl: Labeled) -> int:
    # str field + int field.
    return len(lbl.name) + lbl.value
