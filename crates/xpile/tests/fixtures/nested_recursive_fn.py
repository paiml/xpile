def fact(n: int) -> int:
    def go(k: int) -> int:
        if k <= 1:
            return 1
        return k * go(k - 1)
    return go(n)


def fib(n: int) -> int:
    def go(k: int) -> int:
        if k < 2:
            return k
        return go(k - 1) + go(k - 2)
    return go(n)
