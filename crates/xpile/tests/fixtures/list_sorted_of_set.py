def unique_count(xs: list[int]) -> int:
    # list(set(...)) — unique elements as a list.
    return len(list(set(xs)))


def smallest_unique(xs: list[int]) -> int:
    # sorted(set(...)) — sorted unique elements.
    return sorted(set(xs))[0]


def largest_unique(xs: list[int]) -> int:
    return sorted(set(xs))[-1]


def sorted_desc_first(xs: list[int]) -> int:
    # sorted(set(...), reverse=True).
    return sorted(set(xs), reverse=True)[0]
