# PMAT-644: str.rsplit(sep, maxsplit) — split from the RIGHT, capping at maxsplit
# splits, parts in left-to-right order (like Python). Common for splitting off
# the last component (`name.rsplit(".", 1)`).
def last_two(s: str) -> str:
    return "|".join(s.rsplit("/", 2))


def strip_ext(s: str) -> str:
    return s.rsplit(".", 1)[0]


def no_limit(s: str) -> str:
    return "|".join(s.rsplit(".", -1))  # negative maxsplit = all parts


# bare rsplit(sep) is identical to split(sep) (regression).
def bare(s: str) -> int:
    return len(s.rsplit("."))
