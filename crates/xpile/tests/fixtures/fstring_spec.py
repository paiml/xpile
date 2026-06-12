# PMAT-502am (Tranche 2): f-string format specs -> Rust format! specs.
# Supported subset: .Nf (float), 0Nd/Nd (int width/zero-pad), >N/<N/^N (align).
def price(x: float) -> str:
    return f"${x:.2f}"


def padded(n: int) -> str:
    return f"[{n:05d}]"


def aligned(name: str) -> str:
    return f"|{name:>8}|"


def width(n: int) -> str:
    return f"{n:4d}"
