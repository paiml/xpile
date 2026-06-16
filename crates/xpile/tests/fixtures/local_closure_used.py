def adder(k: int) -> int:
    def inc(x: int) -> int:
        return x + 1

    return inc(k) + inc(k)
