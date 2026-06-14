def index_of(s: str, target: str) -> int:
    # enumerate over a string — iterate (index, 1-char string) pairs.
    for i, c in enumerate(s):
        if c == target:
            return i
    return -1


def weighted_ord(s: str) -> int:
    t = 0
    for i, c in enumerate(s):
        t += i * ord(c)
    return t


def start_sum(s: str) -> int:
    # enumerate(s, start) over a string.
    t = 0
    for i, c in enumerate(s, 1):
        t += i
    return t


def zip_str_list(s: str, ns: list[int]) -> int:
    # zip a string with a list.
    t = 0
    for c, n in zip(s, ns):
        t += ord(c) + n
    return t
