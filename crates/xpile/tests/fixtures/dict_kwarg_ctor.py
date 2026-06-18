# PMAT-811 (HUNT-V22 #9 CC-1): the dict(a=1, b=2) keyword constructor was
# rejected ("dict is not a top-level function" — the keyword-normalizer). Python
# builds {"a": 1, "b": 2} (string keys from the kwarg names); it now lowers to a
# string-keyed dict literal. Cross-checked vs python3.


def int_vals() -> int:
    d = dict(a=1, b=2, c=3)
    return d["a"] + d["b"] + d["c"]


def str_vals() -> str:
    d = dict(host="local", port="8080")
    return d["host"] + ":" + d["port"]
