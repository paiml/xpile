def sq_plus_one(n: int) -> int:
    def helper(x: int) -> int:
        sq = x * x
        return sq + 1

    return helper(n)


def clamped(n: int) -> int:
    def guard(x: int) -> int:
        if x < 0:
            return 0
        return x

    return guard(n)
