# PMAT-1094 adjacent: a vararg on `__enter__` lowers to a required
# `Vec<i64>` parameter, but the desugar calls `__enter__()` with zero args —
# rustc E0061 far from the cause (CPython is fine: the vararg receives `()`).
# Must REFUSE at the `with` site, not surface a downstream compile error.
class CM:
    def __enter__(self, *args: int) -> "CM":
        print("enter")
        return self

    def __exit__(self, a: int, b: int, c: int) -> None:
        print("exit")


def main() -> None:
    with CM():
        print("body")
