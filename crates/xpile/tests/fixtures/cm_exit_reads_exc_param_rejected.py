# PMAT-1084 (skeptic pass PMAT-1081, hole b): `__exit__`'s exc params are
# fabricated ZERO-VALUES by the desugar; CPython passes None on the clean
# path (so `a == 0` is False → prints "exc") while the transpiled zero makes
# `a == 0` True → prints "clean" — a SILENT wrong-branch divergence. An
# `__exit__` that uses its exc params must REFUSE at the `with` site.
class Check:
    def __enter__(self) -> "Check":
        return self

    def __exit__(self, a: int, b: int, c: int) -> None:
        if a == 0:
            print("clean")
        else:
            print("exc")


def work() -> None:
    with Check():
        print("work")
