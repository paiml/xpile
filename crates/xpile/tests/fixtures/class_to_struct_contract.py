from dataclasses import dataclass


@dataclass
class Point:
    x: int
    y: int

    def sum(self) -> int:
        # read-only method → &self; the instance stays usable after the call.
        return self.x + self.y


def make_point(a: int, b: int) -> Point:
    return Point(a, b)


def point_x(p: Point) -> int:
    return p.x


def point_sum_twice(p: Point) -> int:
    # two consecutive &self method calls — instance not moved.
    return p.sum() + p.sum()
