def dot3(a: list[int], b: list[int], c: list[int]) -> int:
    # Three-way parallel iteration: a triple product sum.
    total = 0
    for x, y, z in zip(a, b, c):
        total += x * y * z
    return total


def sum_triples(a: list[int], b: list[int], c: list[int]) -> int:
    s = 0
    for x, y, z in zip(a, b, c):
        s += x + y + z
    return s


def shortest_stops(a: list[int], b: list[int], c: list[int]) -> int:
    # zip stops at the shortest iterable, like Python.
    n = 0
    for x, y, z in zip(a, b, c):
        n += 1
    return n
