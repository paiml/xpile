from dataclasses import dataclass


@dataclass
class Point:
    x: int
    y: int


def seconds_first(ps: list[tuple[int, int]]) -> int:
    # list comp indexing a tuple element — p[1] must lower to `.1`.
    return [p[1] for p in ps][0]


def sum_seconds(ps: list[tuple[int, int]]) -> int:
    # generator expression over tuple elements.
    return sum(p[1] for p in ps)


def count_big(ps: list[tuple[int, int]]) -> int:
    # filter predicate indexing a tuple element.
    return len([p for p in ps if p[1] > 3])


def sum_x(ps: list[Point]) -> int:
    # comprehension reading a struct field of the loop element.
    return sum(p.x for p in ps)
