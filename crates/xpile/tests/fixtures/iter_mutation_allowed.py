# PMAT-1013 companion: the shapes that must STAY accepted next to the
# mutation-during-iteration refusal — mutating a DIFFERENT list in the body,
# mutating the iterated list AFTER the loop, and the PMAT-816 element
# in-place lane (`for row in grid: row.append(...)` mutates the ELEMENT via
# iter_mut, not the iterated spine).
def map_double(xs: list[int]) -> int:
    out: list[int] = []
    for x in xs:
        out.append(x * 2)
    return len(out) + out[0]


def append_after(xs: list[int]) -> int:
    for x in xs:
        pass
    xs.append(9)
    return len(xs)


def grow_rows() -> int:
    grid: list[list[int]] = [[1], [2]]
    for row in grid:
        row.append(0)
    return len(grid[0])
