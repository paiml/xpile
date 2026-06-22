def main() -> None:
    n: int = 100
    steps: int = 0
    while n > 1:
        if n % 2 == 0:
            n = n // 2
        else:
            n = 3 * n + 1
        steps += 1
    print(steps)
