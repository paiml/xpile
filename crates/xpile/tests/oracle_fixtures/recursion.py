def fact(n: int) -> int:
    return 1 if n <= 1 else n * fact(n - 1)
def main() -> None:
    print(fact(5))
    print(fact(10))
