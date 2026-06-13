# PMAT-502cm (Tranche 2): ord(c) (1-char str -> int code point) and
# chr(n) (int -> 1-char str). Previously both emitted an undefined Rust fn.
def code(c: str) -> int:
    return ord(c)


def char(n: int) -> str:
    return chr(n)


def shift(c: str) -> str:
    return chr(ord(c) + 1)
