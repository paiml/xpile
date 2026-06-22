def main() -> None:
    a: int = 1
    b: int = 2
    a, b = b, a
    print(a, b)
    p: tuple[int, int] = (10, 20)
    x, y = p
    print(x + y)
