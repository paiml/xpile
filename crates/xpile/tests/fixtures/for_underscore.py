def repeat_count(n: int) -> int:
    x = 0
    for _ in range(n):
        x += 2
    return x


def sum_indices(n: int) -> int:
    # `_` is a legal (if conventionally unused) binding — read it in the body.
    total = 0
    for _ in range(n):
        total += _
    return total


def nested_grid_count(n: int, m: int) -> int:
    # Nested `for _` need distinct counters; the outer increment must not hit
    # the inner shadow.
    c = 0
    for _ in range(n):
        for _ in range(m):
            c += 1
    return c


def comp_const(n: int) -> list[int]:
    return [7 for _ in range(n)]


def comp_read(n: int) -> int:
    xs = [_ * _ for _ in range(n)]
    return sum(xs)


def set_comp_size(n: int) -> int:
    s = {_ % 3 for _ in range(n)}
    return len(s)
