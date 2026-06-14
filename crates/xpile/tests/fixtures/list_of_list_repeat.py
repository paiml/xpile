def grid(n: int) -> int:
    # `[[0]] * n` — a list of lists; slice repeat needs Copy (Vec isn't),
    # so this must clone-repeat (was an E0277 rustc error).
    g = [[0]] * n
    return len(g)


def grid_cells(n: int) -> int:
    g = [[0, 0]] * n
    return len(g) * len(g[0])


def int_repeat(n: int) -> int:
    xs = [7] * n
    return xs[0] + xs[n - 1]


def str_repeat(n: int) -> str:
    return "ab" * n
