def map_pair_sum_first(ps: list[tuple[int, int]]) -> int:
    # map lambda indexing tuple elements — p[0]/p[1] must lower to .0/.1.
    return list(map(lambda p: p[0] + p[1], ps))[0]


def filter_big_count(ps: list[tuple[int, int]]) -> int:
    # filter predicate indexing a tuple element.
    return len(list(filter(lambda p: p[1] > 3, ps)))


def map_pick_second(ps: list[tuple[int, int]]) -> int:
    return list(map(lambda p: p[1], ps))[2]
