# PMAT-1037 slice D: attribute-chain container mutators on struct LOCALS —
# `b.items.append(x)` OUTSIDE methods previously hit the subprocess
# recognizer with a factually wrong diagnostic (the PMAT-1022 branch was
# self-only; `self` is just the method case of a struct-typed Name).
# Covered: plain appends on a local's list field, loop-body appends
# (pre-walk marks the root `let mut`), int→float widening of the pushed
# value, and a REUSED pushed value riding clone_if_reused_non_copy
# (ListAppend parity — was E0382). Non-append field mutators keep their
# precise refusal; the alias guard covers the chain (see the reject twin).
# Differentially verified vs CPython (MATCH '11\n9\n2.5\n4').
class Acc:
    vals: list[int]
    xs: list[float]

    def __init__(self) -> None:
        self.vals = []
        self.xs = [0.5]

    def size(self) -> int:
        return len(self.vals)


def append_outside_methods() -> int:
    a = Acc()
    a.vals.append(4)
    a.vals.append(9)
    return a.size() + a.vals[1]


def append_in_loop() -> int:
    a = Acc()
    for i in range(4):
        a.vals.append(i * i)
    return a.vals[3]


def append_widens() -> float:
    a = Acc()
    a.xs.append(2)
    return a.xs[0] + a.xs[1]


class Bag:
    items: list[str]

    def __init__(self) -> None:
        self.items = []


def append_reused_value() -> int:
    b = Bag()
    w = "ab"
    b.items.append(w)
    b.items.append(w)
    return len(b.items) + len(w)

