# PMAT-1166: membership `x in <container>` tests via `==` against the container's
# elements, so `Obj(1) in [Obj(1)]` over a PLAIN class (identity `==` in Python)
# is False in CPython but xpile's structural `Vec::contains` returns true — a
# silent divergence. Refused fail-loud (mirrors the container `==` refusal).
class Obj:
    def __init__(self, x: int) -> None:
        self.x = x


def member() -> bool:
    xs = [Obj(1)]
    return Obj(1) in xs
