def pick(i: int, opts: list[int] = [10, 20, 30]) -> int:
    return opts[i]


def run() -> int:
    return pick(1) + pick(2)
