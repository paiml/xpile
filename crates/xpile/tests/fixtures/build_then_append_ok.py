# PMAT-1046 companion: the build-then-append idiom (mutate the local BEFORE
# embedding it) MUST stay valid — the guard is position-sensitive. Clone
# captures the final state; nothing mutates the local afterward. MATCH.
def build_rows() -> int:
    grid: list[list[int]] = []
    for i in range(3):
        row: list[int] = []
        row.append(i)
        row.append(i * 2)
        grid.append(row)
    return grid[2][0] + grid[2][1] + len(grid)
