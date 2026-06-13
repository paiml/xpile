def part(s: str, sep: str) -> tuple[str, str, str]:
    return s.partition(sep)


def rpart(s: str, sep: str) -> tuple[str, str, str]:
    return s.rpartition(sep)
