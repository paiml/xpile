def products(n: int) -> int:
    # 2-generator list comp over two ranges (Cartesian product).
    r = [i * j for i in range(n) for j in range(n)]
    return sum(r)


def off_diagonal(n: int) -> int:
    # 2-gen with an inner filter.
    r = [i * j for i in range(n) for j in range(n) if i != j]
    return sum(r)


def mixed(ys: list[int]) -> int:
    # outer range, inner list.
    r = [i * y for i in range(3) for y in ys]
    return sum(r)


def grid_size(n: int) -> int:
    # 2-gen dict comp over ranges.
    d = {i * 10 + j: i for i in range(n) for j in range(n)}
    return len(d)
