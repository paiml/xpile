# PMAT-676: `"{:x}".format(n)` formats negatives SIGN-MAGNITUDE in Python
# (`"{:x}".format(-255)` == "-ff"), not two's-complement like Rust's `{:x}`.
def hexfmt(n: int) -> str:
    return "{:x}".format(n)


def octfmt(n: int) -> str:
    return "{:o}".format(n)


def binfmt(n: int) -> str:
    return "{:b}".format(n)


def hexup(n: int) -> str:
    return "{:X}".format(n)


def mixed(a: int, b: int) -> str:
    return "{:x} and {:b}".format(a, b)


def with_text(n: int) -> str:
    return "val=0x{:x}!".format(n)
