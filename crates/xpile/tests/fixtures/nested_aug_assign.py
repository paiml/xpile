def diag_accumulate(n: int) -> int:
    # 2D grid built by comprehension, accumulated on the diagonal.
    grid = [[0] * n for _ in range(n)]
    for i in range(n):
        grid[i][i] += i + 1
    return grid[2][2] + grid[1][1]


def histogram() -> int:
    # 2D literal grid + repeated augmented increments (needs `let mut`).
    counts = [[0, 0], [0, 0]]
    counts[0][1] += 5
    counts[0][1] += 2
    counts[1][0] += 9
    return counts[0][1] + counts[1][0]


def cube_scale() -> int:
    # 3D augmented multiply.
    g = [[[1, 1], [1, 1]], [[1, 1], [1, 1]]]
    g[1][0][1] *= 7
    return g[1][0][1]


def single_list_aug() -> int:
    # Regression: single-level `xs[i] += v` on a literal list.
    xs = [10, 20, 30]
    xs[1] += 5
    return xs[1]


def single_dict_aug() -> int:
    # Regression: single-level `d[k] += v` on a literal dict.
    d = {1: 100}
    d[1] += 7
    return d[1]
