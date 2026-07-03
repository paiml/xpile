# PMAT-1084 (skeptic pass PMAT-1081, hole a): a truthy `__exit__` return
# SUPPRESSES the in-flight exception in CPython — `run()` returns "after".
# The desugared finally-only `__exit__` call discards the return value, so
# the transpiled code would crash (exit 101) instead: a silent divergence
# on the suppression path. Must REFUSE at the `with` site.
class Guard:
    def __enter__(self) -> "Guard":
        return self

    def __exit__(self, a: int, b: int, c: int) -> bool:
        return True


def run() -> str:
    with Guard():
        raise ValueError("boom")
    return "after"
