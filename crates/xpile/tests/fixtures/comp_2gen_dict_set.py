def grid(a: list[int], b: list[int]) -> dict[int, int]:
    # Two-generator dict comprehension → nested loops inserting into the dict.
    return {x * 10 + y: x + y for x in a for y in b}


def sums(a: list[int], b: list[int]) -> set[int]:
    # Two-generator set comprehension → nested loops adding to the set.
    return {x + y for x in a for y in b}


def filtered_set(a: list[int], b: list[int]) -> set[int]:
    # Per-generator filters apply to their own loop.
    return {x * y for x in a if x > 0 for y in b if y > 0}
