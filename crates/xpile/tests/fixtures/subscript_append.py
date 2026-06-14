def grid_row_append(g: list[list[int]], i: int) -> int:
    # append on a list-of-list subscript receiver: g[i].push(...).
    g[i].append(99)
    return len(g[i])


def first_row_total(g: list[list[int]]) -> int:
    g[0].append(10)
    g[0].append(20)
    return sum(g[0])


def bucket_append(d: dict[str, list[int]], k: str, v: int) -> int:
    # append on a dict-of-list subscript receiver: d.get_mut(&k).unwrap().push(v).
    d[k].append(v)
    return len(d[k])
