# PMAT-502ch (Tranche 2): str.format with format specs `{:.2f}` / `{:05d}`.
# Each spec is translated to its Rust form by the arg's type; float args are
# admitted when they carry a `.Nf` spec. Positional + spec also works.
def money(x: float) -> str:
    return "${:.2f}".format(x)


def padded(n: int) -> str:
    return "id={:05d}".format(n)


def both(a: float, b: int) -> str:
    return "{:.1f} ({:03d})".format(a, b)
