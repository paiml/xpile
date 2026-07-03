# PMAT-1094 keep-green boundary: DEFAULTED dunder params are the accepted
# side of the signature honesty line. `__enter__(self, lvl=3)` — CPython
# binds the default; call-site default filling emits `__enter__(3)`
# faithfully. A defaulted 4th `__exit__` param binds its default in CPython
# while the desugar's fabricated zero is unread-unobservable (a READ of any
# exc param refuses via PMAT-1084). CPython-verified: doubled_via_cm() == 42.
class Gate:
    v: int

    def __init__(self, v: int) -> None:
        self.v = v

    def __enter__(self, lvl: int = 3) -> "Gate":
        return self

    def __exit__(self, a: int, b: int, c: int, d: int = 9) -> None:
        pass

    def doubled(self) -> int:
        return self.v * 2


def doubled_via_cm() -> int:
    result: int = 0
    with Gate(21) as g:
        result = g.doubled()
    return result
