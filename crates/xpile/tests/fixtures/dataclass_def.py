from dataclasses import dataclass


@dataclass
class Point:
    x: int
    y: int


@dataclass
class Tagged:
    label: str
    count: int
    ratio: float
    items: list[int]
