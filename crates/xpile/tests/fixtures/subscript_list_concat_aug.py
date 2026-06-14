# PMAT-604: `grid[i] += [..]` over a nested list is list concatenation, but the
# subscript aug-assign routed `+` through integer checked_add on a Vec (E0599).
# The flat `xs += [..]` case was already correct (ListExtend); the indexed /
# nested form now also concatenates.
def append_row(grid: list[list[int]]) -> int:
    grid[0] += [10, 20]
    return len(grid[0]) * 100 + grid[0][1]


def extend_inner(cube: list[list[list[int]]]) -> int:
    cube[0][1] += [7, 8]
    return len(cube[0][1])
