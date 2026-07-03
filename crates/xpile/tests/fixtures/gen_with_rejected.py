# PMAT-1093 (skeptic pass PMAT-1090, B-F9-with-machinery): a `with` block in
# a generator body desugars to `__enter__`/`__exit__` calls whose side
# effects run at MATERIALIZATION time eagerly, and `__exit__` timing relative
# to the consumer differs from CPython's lazy protocol (CPython holds the
# context open across the yield until the consumer resumes). Refuses.
class Res:
    def __init__(self) -> None:
        self.x: int = 5

    def __enter__(self) -> int:
        return self.x

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        return None


def gen(n: int) -> int:
    for i in range(n):
        with Res() as r:
            yield r + i


def entry() -> int:
    return sum(gen(2))
