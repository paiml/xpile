# PMAT-1094 adjacent: a REQUIRED 4th `__exit__` param cannot bind CPython's
# three-argument call — `TypeError: CM.__exit__() missing 1 required
# positional argument: 'd'` at the first `with` exit (CPython 3.10, exit
# code 1). The fabricated-zeros desugar happily passes 4 zeros and runs —
# a SILENT divergence. Must REFUSE. (A DEFAULTED 4th param stays accepted:
# CPython binds its default, the fabricated zero is unread-unobservable,
# and reads refuse via the PMAT-1084 exc-param scan.)
class CM:
    def __enter__(self) -> "CM":
        print("enter")
        return self

    def __exit__(self, a: int, b: int, c: int, d: int) -> None:
        print("exit")


def main() -> None:
    with CM():
        print("body")
