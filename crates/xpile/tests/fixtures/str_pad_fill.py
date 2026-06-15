# PMAT-632: optional fill-char arg for str.rjust/ljust/center.
def pad_r(s: str, w: int) -> str:
    return s.rjust(w, "*")


def pad_l(s: str, w: int) -> str:
    return s.ljust(w, "*")


def pad_c(s: str, w: int) -> str:
    return s.center(w, "-")


def lit_pad() -> str:
    return "5".rjust(4, "0")
