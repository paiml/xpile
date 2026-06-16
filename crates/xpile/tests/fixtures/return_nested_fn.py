def make_const(k: int):
    def inner() -> int:
        return k

    return inner
