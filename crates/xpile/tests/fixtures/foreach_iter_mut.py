# PMAT-816 (HUNT-V21 #3/4/8): a for-loop that mutates each element in place
# (row.append(x), row[i] = v) emitted `for row in grid.iter().cloned()`, so the
# mutation hit a discarded clone AND `row` was not mut (rustc E0596). The loop
# now binds `row` by &mut via `grid.iter_mut()` (grid marked mut), so the
# mutation reaches the original. A read-only loop keeps the cloned form.
# Cross-checked vs python3.


def grow(grid: list[list[int]]) -> int:
    for row in grid:
        row.append(0)
    total = 0
    for row in grid:
        total = total + len(row)
    return total


def set_first(grid: list[list[int]]) -> int:
    for row in grid:
        row[0] = 99
    return grid[0][0] + grid[1][0]


def read_only(grid: list[list[int]]) -> int:
    total = 0
    for row in grid:
        total = total + len(row)
    return total
