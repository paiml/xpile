def main() -> None:
    xs: list[int] = [3, 1, 2]
    xs.sort()
    print(xs)
    doubled: list[int] = [x * 2 for x in xs]
    print(doubled)
    print(sum(doubled))
    print(len(xs))
