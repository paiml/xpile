# PMAT-502bj (Tranche 2): module-level int/bool/float constants.
MAX = 100
NEG = -5
FLAG = True
RATIO = 2.5


def get_max() -> int:
    return MAX


def use_neg(x: int) -> int:
    return x + NEG


def use_flag() -> bool:
    return FLAG


def scaled(x: float) -> float:
    return x * RATIO
