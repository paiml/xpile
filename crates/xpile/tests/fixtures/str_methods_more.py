# PMAT-502l (Tranche 2): more string methods — lstrip/rstrip (Str) and
# find/count (Int).
def trim_left(s: str) -> str:
    return s.lstrip()


def trim_right(s: str) -> str:
    return s.rstrip()


def index_of(s: str, sub: str) -> int:
    return s.find(sub)


def occurrences(s: str, sub: str) -> int:
    return s.count(sub)
