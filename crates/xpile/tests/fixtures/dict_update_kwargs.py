# PMAT-850 (HUNT-V27 #18): d.update(a=1, b=2) keyword form — Python uses the kwarg
# names as string keys (like dict(a=1)) — was clean-rejected. It now builds a dict
# literal from the kwargs and merges it (str-keyed dicts only). A positional
# d.update(<dict>) is unchanged. Cross-checked vs python3.


def kwargs_update() -> int:
    d: dict[str, int] = {"a": 1}
    d.update(a=10, b=2, c=3)
    return d["a"] * 100 + d["b"] * 10 + d["c"]


def positional_update(d: dict[str, int], o: dict[str, int]) -> int:
    d.update(o)
    return len(d)
