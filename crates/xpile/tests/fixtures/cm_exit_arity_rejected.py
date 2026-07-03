# PMAT-1094 adjacent: CPython calls `__exit__(exc_type, exc_val, exc_tb)` —
# a zero-param `__exit__` raises `TypeError: CM.__exit__() takes 1 positional
# argument but 4 were given` at the first `with` exit (CPython 3.10: enter,
# body, TypeError, exit code 1; `exit` never prints). The fabricated-zeros
# desugar matches the declared arity and runs happily (enter, body, exit,
# exit code 0) — a SILENT divergence. Must REFUSE at the `with` site.
class CM:
    def __enter__(self) -> "CM":
        print("enter")
        return self

    def __exit__(self) -> None:
        print("exit")


def main() -> None:
    with CM():
        print("body")
