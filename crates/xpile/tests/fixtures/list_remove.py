def remove_count() -> int:
    xs = [1, 2, 3, 2]
    xs.remove(2)
    return len(xs)


def remove_first_only() -> int:
    # `.remove` deletes only the FIRST matching element.
    xs = [1, 2, 3, 2]
    xs.remove(2)
    return xs[2]


def remove_then_sum() -> int:
    xs = [10, 20, 30]
    xs.remove(20)
    total = 0
    for x in xs:
        total += x
    return total


def remove_str() -> int:
    words = ["a", "b", "c"]
    words.remove("b")
    return len(words)


def remove_param(xs: list[int], k: int) -> int:
    xs.remove(k)
    return len(xs)
