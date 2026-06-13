def union_splat(a: set[int], b: set[int]) -> int:
    return len({*a, *b})


def splat_with_elem(a: set[int]) -> int:
    return len({*a, 9})


def elem_then_splat(a: set[int]) -> int:
    return len({9, *a, 1})


def lone_splat_is_copy(a: set[int]) -> int:
    # {*a} is a shallow copy — mutating it must not touch `a`.
    b = {*a}
    b.add(99)
    return len(a) * 10 + len(b)
