def insert_absent() -> int:
    d = {1: 10}
    d.setdefault(2, 20)
    return d[2]


def keep_present() -> int:
    # setdefault must NOT overwrite an existing key.
    d = {1: 10}
    d.setdefault(1, 99)
    return d[1]


def init_in_loop(keys: list[int]) -> int:
    # the canonical "ensure each key exists" loop idiom.
    d = {0: 0}
    for k in keys:
        d.setdefault(k, 0)
    return len(d)


def str_keys() -> int:
    d = {"a": 1}
    d.setdefault("b", 2)
    return d["b"]
