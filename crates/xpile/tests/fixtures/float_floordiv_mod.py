# PMAT-502br (Tranche 2): float floor-division `//` and modulo `%`.
# Python floor semantics: `a // b` = floor(a/b), `a % b` follows the
# divisor's sign — both differ from Rust's truncating `/` and `%`.
def fd(a: float, b: float) -> float:
    return a // b


def fmod(a: float, b: float) -> float:
    return a % b


def wrap(x: float, period: float) -> float:
    x %= period
    return x
