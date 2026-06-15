# PMAT-656: max()/min()/sum() over a dict iterate its KEYS in Python. A bare
# dict arg emitted an undefined free call (max(d) → E0425); it now materializes
# to the keys list, like max(d.keys()).


def max_dict(d: dict[int, int]) -> int:
    return max(d)


def min_dict(d: dict[int, int]) -> int:
    return min(d)


def sum_dict(d: dict[int, int]) -> int:
    return sum(d)


def max_str_keys(d: dict[str, int]) -> str:
    return max(d)


def max_dict_keys_regression(d: dict[int, int]) -> int:
    # explicit .keys() still works
    return max(d.keys())
