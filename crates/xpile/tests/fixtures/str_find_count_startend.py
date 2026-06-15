# PMAT-675: str `.find(sub, start[, end])` / `.count(sub, start[, end])` — search
# within the char-slice `s[start:end]`. find returns the char index in the
# ORIGINAL string (or -1); count the number of non-overlapping occurrences.
def find2(s: str, sub: str, start: int) -> int:
    return s.find(sub, start)


def find3(s: str, sub: str, start: int, end: int) -> int:
    return s.find(sub, start, end)


def count2(s: str, sub: str, start: int) -> int:
    return s.count(sub, start)


def count3(s: str, sub: str, start: int, end: int) -> int:
    return s.count(sub, start, end)


def find_neg(s: str, sub: str) -> int:
    return s.find(sub, -3)
