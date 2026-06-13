def diag_fill(n: int) -> int:
    grid = [[0] * n for r in range(n)]
    for i in range(n):
        grid[i][i] = i + 1
    return grid[2][2] + grid[0][0]


def cube_set(g: list[list[list[int]]], i: int, j: int, k: int, v: int) -> int:
    g[i][j][k] = v
    return g[i][j][k]
