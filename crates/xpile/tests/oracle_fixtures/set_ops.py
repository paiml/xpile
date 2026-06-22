def main() -> None:
    a: set[int] = {1, 2, 3}
    b: set[int] = {2, 3, 4}
    print(len(a | b))
    print(len(a & b))
    print(2 in a)
    print(5 in a)
