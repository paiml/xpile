# PMAT-641: a runtime-negative index at ANY nesting level of a subscript WRITE
# wraps like Python (`grid[-1][-1] = v`), each level using its own len. Completes
# the negative-index work (read PMAT-639, single write PMAT-640, nested here).
def set_outer(grid: list[list[int]], i: int) -> int:
    grid[i][0] = 99
    return grid[1][0]


def set_inner(grid: list[list[int]], j: int) -> int:
    grid[0][j] = 88
    return grid[0][1]


def set_both_neg(grid: list[list[int]]) -> int:
    grid[-1][-1] = 77
    return grid[1][1]


# All-non-negative-literal nested write is unchanged (regression guard).
def set_literal(grid: list[list[int]]) -> int:
    grid[1][0] = 50
    return grid[1][0]
