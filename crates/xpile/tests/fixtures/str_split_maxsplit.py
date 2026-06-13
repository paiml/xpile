def key_part(s: str) -> str:
    # split(sep, maxsplit) → at most maxsplit+1 parts.
    return s.split("=", 1)[0]


def value_part(s: str) -> str:
    return s.split("=", 1)[1]


def field_count(s: str) -> int:
    # 1-arg split (unbounded) is unchanged.
    return len(s.split(","))


def capped_count(s: str) -> int:
    # maxsplit=2 → at most 3 parts.
    return len(s.split(",", 2))
