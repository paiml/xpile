def add_one(x: int) -> int:
    def inner(y: int) -> int:
        return y + 1

    return inner(x)


def double_twice(x: int) -> int:
    def dbl(y: int) -> int:
        return y * 2

    return dbl(x) + dbl(x)


def shout(s: str) -> str:
    def up(t: str) -> str:
        return t.upper()

    return up(s)
