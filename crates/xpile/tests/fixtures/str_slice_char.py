def mid(s: str) -> str:
    # Python string slices index by characters (this panicked on non-ASCII).
    return s[1:3]


def prefix(s: str, n: int) -> str:
    return s[:n]


def suffix(s: str) -> str:
    return s[1:]


def drop_last(s: str) -> str:
    return s[:-1]


def from_neg(s: str) -> str:
    return s[-2:]


def oob(s: str) -> str:
    return s[1:100]


def ascii_slice(s: str) -> str:
    return s[1:4]


def list_slice(xs: list[int]) -> int:
    ys = xs[1:3]
    return ys[0]
