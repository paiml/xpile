# PMAT-502aw (Tranche 2): str padding s.rjust(w) / s.ljust(w).
def pad_r(s: str, w: int) -> str:
    return s.rjust(w)


def pad_l(s: str, w: int) -> str:
    return s.ljust(w)


def lit_pad() -> str:
    return "hi".rjust(5)
