def first_char(s: str) -> str:
    # sorted(s) sorts the characters → a list of 1-char strings.
    cs = sorted(s)
    return cs[0]


def sorted_joined(s: str) -> str:
    return "".join(sorted(s))


def first_char_desc(s: str) -> str:
    cs = sorted(s, reverse=True)
    return cs[0]


def char_count(s: str) -> int:
    return len(sorted(s))
