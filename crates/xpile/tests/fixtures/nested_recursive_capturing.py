def outer(n: int) -> int:
    base = 10
    def go(k: int) -> int:
        if k <= 0:
            return base
        return go(k - 1) + base
    return go(n)
