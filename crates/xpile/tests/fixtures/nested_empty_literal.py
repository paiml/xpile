def groups() -> dict[int, list[int]]:
    d: dict[int, list[int]] = {0: [], 1: []}
    d[0].append(5)
    d[1].append(9)
    d[1].append(8)
    return d


def matrix() -> list[list[int]]:
    m: list[list[int]] = [[], [1], []]
    m[0].append(7)
    return m


def str_groups() -> dict[str, list[str]]:
    d: dict[str, list[str]] = {"a": [], "b": ["x"]}
    d["a"].append("hello")
    return d
