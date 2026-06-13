def replace_first(s: str) -> str:
    # 3-arg replace: only the first occurrence.
    return s.replace("a", "X", 1)


def replace_two(s: str) -> str:
    return s.replace("o", "0", 2)


def replace_all(s: str) -> str:
    # 2-arg form is unchanged.
    return s.replace("z", "Z")
