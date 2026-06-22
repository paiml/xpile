def addd(a: int, b: int = 2) -> int:
    return a + b


def call_it() -> int:
    xs = [1]
    return addd(*xs)
