def accumulate(x: int, acc: list[int] = [0]) -> int:
    acc.append(x)
    return len(acc)
