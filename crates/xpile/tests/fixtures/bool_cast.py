# PMAT-502be (Tranche 2): bool(x) truthiness cast.
def from_int(x: int) -> bool:
    return bool(x)


def from_str(s: str) -> bool:
    return bool(s)


def from_list(xs: list[int]) -> bool:
    return bool(xs)


def idempotent(b: bool) -> bool:
    return bool(b)
