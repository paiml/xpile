from __future__ import annotations

from dataclasses import dataclass


@dataclass
class Point:
    x: int
    y: int

    @classmethod
    def origin(cls) -> Point:
        # `cls(...)` constructs the enclosing class.
        return cls(0, 0)

    @classmethod
    def diagonal(cls, n: int) -> Point:
        return cls(n, n)

    @staticmethod
    def unit() -> Point:
        # A classmethod and a staticmethod may both build the class; a static
        # method names the class explicitly.
        return Point.diagonal(1)

    def manhattan(self) -> int:
        return self.x + self.y


def origin_sum() -> int:
    return Point.origin().manhattan()


def diagonal_sum(n: int) -> int:
    return Point.diagonal(n).manhattan()


def unit_sum() -> int:
    return Point.unit().manhattan()
