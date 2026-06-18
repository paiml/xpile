# PMAT-764 (HUNT-V16 #4): a non-negative LITERAL list index out of bounds
# (`data[10]`) panicked with Rust's NATIVE message, not `xpile: IndexError:`,
# so inside a try it was silently swallowed by an unrelated typed except
# (`except KeyError:` caught it) where Python propagates the IndexError. The
# literal-index path now bounds-checks with the tagged panic (mirroring the
# runtime/negative path, PMAT-744). Cross-checked vs python3.


def wrong_except() -> int:
    data = [1, 2, 3]
    try:
        return data[10]
    except KeyError:
        return -1


def right_except() -> int:
    data = [1, 2, 3]
    try:
        return data[10]
    except IndexError:
        return -2


def in_bounds() -> int:
    data = [10, 20, 30]
    return data[1]
