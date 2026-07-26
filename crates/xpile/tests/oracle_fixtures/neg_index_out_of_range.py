# PMAT-1351: a negative-integer-LITERAL index that is OUT OF RANGE.
#
# `xs[-4]` on a 3-element list raises IndexError in CPython. The frontend used
# to desugar the literal to `len(xs) - 4`, which every backend's index path then
# normalized a SECOND time (`len + (len - 4)` = 2), so the read/store/aug paths
# silently used `xs[2]` and the del/pop paths reached `(-1) as usize` =
# usize::MAX and panicked with an UNTAGGED native message that no typed
# `except IndexError` could catch.
#
# Each arm below both VALUE-checks the in-range case and proves the out-of-range
# case raises a CATCHABLE IndexError — the tag, not just the panic. The mutating
# arms are split into helpers because the v0.2.0 `try` lowering accepts exactly
# `try: return <expr> except E: return <expr>`.


def read_last(xs: list[int]) -> int:
    return xs[-1]


def read_oob(xs: list[int]) -> int:
    try:
        return xs[-4]
    except IndexError:
        return -1


def read_len_relative(xs: list[int]) -> int:
    # NOT the same program as xs[-4]: CPython evaluates `3 - 4` = -1 first, then
    # wraps that to the tail. Legal, and it must NOT raise.
    return xs[len(xs) - 4]


def store_then_read(xs: list[int]) -> int:
    xs[-4] = 99
    return xs[0]


def store_oob(xs: list[int]) -> int:
    try:
        return store_then_read(xs)
    except IndexError:
        return -1


def aug_then_read(xs: list[int]) -> int:
    xs[-4] += 10
    return xs[0]


def aug_oob(xs: list[int]) -> int:
    try:
        return aug_then_read(xs)
    except IndexError:
        return -1


def del_last(xs: list[int]) -> int:
    del xs[-1]
    return len(xs)


def del_then_len(xs: list[int]) -> int:
    del xs[-4]
    return len(xs)


def del_oob(xs: list[int]) -> int:
    try:
        return del_then_len(xs)
    except IndexError:
        return -1


def pop_last(xs: list[int]) -> int:
    return xs.pop(-1)


def pop_oob(xs: list[int]) -> int:
    try:
        return xs.pop(-4)
    except IndexError:
        return -1


def pop_one_element(xs: list[int]) -> int:
    # The n < k <= 2n corner: on a 1-element list, `-2` normalizes to -1 and
    # must raise. Under the old double-normalize it popped slot 0.
    try:
        return xs.pop(-2)
    except IndexError:
        return -1


def main() -> None:
    print(read_last([1, 2, 3]))
    print(read_oob([1, 2, 3]))
    print(read_len_relative([1, 2, 3]))
    print(store_oob([1, 2, 3]))
    print(aug_oob([1, 2, 3]))
    print(del_last([1, 2, 3]))
    print(del_oob([1, 2, 3]))
    print(pop_last([1, 2, 3]))
    print(pop_oob([1, 2, 3]))
    print(pop_one_element([5]))
