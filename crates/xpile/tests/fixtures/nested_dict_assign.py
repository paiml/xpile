def dict_dict(v: int) -> int:
    d: dict[str, dict[str, int]] = {"a": {"b": 1}}
    d["a"]["b"] = v
    return d["a"]["b"]


def dict_list(v: int) -> int:
    d: dict[str, list[int]] = {"a": [1, 2, 3]}
    d["a"][1] = v
    return d["a"][1]


def dict_list_neg(v: int) -> int:
    d: dict[str, list[int]] = {"a": [1, 2, 3]}
    d["a"][-1] = v
    return d["a"][2]


def three_level(v: int) -> int:
    d: dict[str, dict[str, list[int]]] = {"a": {"b": [9]}}
    d["a"]["b"][0] = v
    return d["a"]["b"][0]


def list_list_regression(v: int) -> int:
    g: list[list[int]] = [[1, 2], [3, 4]]
    g[1][0] = v
    return g[1][0]
