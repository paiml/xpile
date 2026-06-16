def part_ok(s: str) -> str:
    a, b, c = s.partition("-")
    return a + "|" + b + "|" + c


def rpart_ok(s: str) -> str:
    a, b, c = s.rpartition("-")
    return a + "|" + b + "|" + c


def part_absent(s: str) -> str:
    a, b, c = s.partition("Z")
    return a + "|" + b + "|" + c


def part_empty(s: str, sep: str) -> str:
    a, b, c = s.partition(sep)
    return a + "|" + b + "|" + c
