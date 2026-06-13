def drain_count(xs: list[int]) -> int:
    # bare `xs.pop()` statement (discard the popped value) in a while loop.
    n = 0
    while xs:
        xs.pop()
        n += 1
    return n


def pop_front_len(xs: list[int]) -> int:
    # bare `xs.pop(0)` statement (remove at index, discard).
    xs.pop(0)
    return len(xs)


def pop_twice_sum(xs: list[int]) -> int:
    # mix bare pop with value-position pop.
    xs.pop()
    last = xs.pop()
    return last + len(xs)
