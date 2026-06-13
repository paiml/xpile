def list_copy() -> int:
    # the copy is independent — mutating it must not touch the original.
    xs = [1, 2, 3]
    ys = xs.copy()
    ys.append(99)
    return len(xs) * 10 + len(ys)


def dict_copy() -> int:
    d = {1: 10}
    e = d.copy()
    e[2] = 20
    return len(d) * 10 + len(e)


def set_copy() -> int:
    s = {1, 2}
    t = s.copy()
    t.add(9)
    return len(s) * 10 + len(t)


def copy_param(xs: list[int]) -> int:
    ys = xs.copy()
    ys.append(0)
    return len(ys)
