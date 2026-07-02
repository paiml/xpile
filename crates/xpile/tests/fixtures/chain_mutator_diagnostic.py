# PMAT-1051 (sweep #12): a container mutator through a DEEP receiver chain —
# `o.inner.items.append(9)` (two attribute levels) — reaches the
# subprocess-recognizer fall-through and must refuse with a PRECISE message
# naming the unsupported receiver shape, NOT the factually wrong "only
# subprocess.run([...]) is recognised" (the PMAT-989/1027 honest-diagnostics
# posture). PMAT-1052 handles the single-field `obj.field[i].append`; a
# two-level chain stays refused (precisely).
class Inner:
    items: list[int]

    def __init__(self) -> None:
        self.items = []


class Outer:
    inner: Inner

    def __init__(self) -> None:
        self.inner = Inner()


def main() -> None:
    o: Outer = Outer()
    o.inner.items.append(9)
    print(len(o.inner.items))
