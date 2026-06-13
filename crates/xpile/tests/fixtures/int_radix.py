# PMAT-502cv (Tranche 2): hex(n) / oct(n) / bin(n) — int -> radix string with
# the 0x/0o/0b prefix, sign first for negatives (hex(-255) == "-0xff").
# Previously these emitted undefined Rust functions (silent miscompile).
def h(n: int) -> str:
    return hex(n)


def b(n: int) -> str:
    return bin(n)


def o(n: int) -> str:
    return oct(n)
