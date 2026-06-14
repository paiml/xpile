def ord_sum(s: str) -> int:
    # list comprehension over a string — iterate its characters.
    return sum([ord(c) for c in s])


def upper_count(s: str) -> int:
    # list comp with a string-method body + filter.
    return len([c.upper() for c in s if c != "a"])


def distinct_chars(s: str) -> int:
    # set comprehension over a string.
    return len({c for c in s})


def char_codes(s: str) -> int:
    # dict comprehension over a string.
    d = {c: ord(c) for c in s}
    return len(d)


def digit_count(s: str) -> int:
    # genexpr over a string with a predicate filter.
    return sum(1 for c in s if c.isdigit())
