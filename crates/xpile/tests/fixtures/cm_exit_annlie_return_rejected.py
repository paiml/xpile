# PMAT-1084 (hole a, annotation-lie variant): the return ANNOTATION says
# `-> None` but the body returns True — CPython ignores annotations and
# still suppresses the exception. Detection must scan the body for value
# returns, not trust the annotation. Must REFUSE at the `with` site.
class Sneaky:
    def __enter__(self) -> "Sneaky":
        return self

    def __exit__(self, a: int, b: int, c: int) -> None:
        return True


def run() -> str:
    with Sneaky():
        raise ValueError("boom")
    return "after"
