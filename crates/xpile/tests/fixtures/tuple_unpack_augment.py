def two_accumulators(xs: list[int]) -> int:
    # init two accumulators by tuple-unpack, then augment both.
    lo, hi = 0, 0
    for x in xs:
        lo += x
        hi += x * 2
    return lo + hi


def while_accumulate(n: int) -> int:
    i, total = 0, 0
    while i < n:
        total += i
        i += 1
    return total


def one_mut_one_const() -> int:
    # only `a` is mutated — `b` stays immutable (no unused-mut).
    a, b = 10, 20
    a += 1
    return a + b
