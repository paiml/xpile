def pair_sum(n: int) -> int:
    # Two-generator generator expression consumed by sum().
    return sum(i * j for i in range(n) for j in range(n))


def filtered(n: int) -> int:
    # Two generators with an `if` filter on the inner clause.
    return sum(i + j for i in range(n) for j in range(n) if i != j)


def count_pairs(n: int) -> int:
    # Expression-position two-generator list comprehension (len over it).
    return len([i * 10 + j for i in range(n) for j in range(n)])


def over_lists(a: list[int], b: list[int]) -> int:
    # Two generators over list iterables (a Cartesian dot-product-ish sum).
    return sum(x * y for x in a for y in b)
