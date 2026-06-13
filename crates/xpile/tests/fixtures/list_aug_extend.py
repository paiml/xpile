def extend_literal() -> int:
    # `xs += [..]` is Python's in-place list extend, not numeric add.
    xs = [1, 2]
    xs += [4, 5]
    return len(xs)


def extend_var(ys: list[int]) -> int:
    xs = [1, 2]
    xs += ys
    return len(xs)


def extend_sum() -> int:
    xs = [1, 2]
    xs += [10, 20]
    total = 0
    for x in xs:
        total += x
    return total


def extend_strings() -> int:
    words = ["a", "b"]
    words += ["c", "d", "e"]
    return len(words)
