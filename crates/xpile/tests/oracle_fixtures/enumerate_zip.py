def main() -> None:
    xs: list[int] = [10, 20, 30]
    for i, v in enumerate(xs):
        print(i, v)
    ys: list[int] = [1, 2, 3]
    total: int = 0
    for a, b in zip(xs, ys):
        total += a * b
    print(total)
