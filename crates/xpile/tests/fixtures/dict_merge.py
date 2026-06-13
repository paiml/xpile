def merged_size(a: dict[str, int], b: dict[str, int]) -> int:
    return len({**a, **b})


def merged_get(a: dict[str, int], b: dict[str, int], k: str) -> int:
    d = {**a, **b}
    return d[k]


def merge3(a: dict[str, int], b: dict[str, int], c: dict[str, int]) -> int:
    return len({**a, **b, **c})
