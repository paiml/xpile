# PMAT-502k (Tranche 2): sequence repetition seq * n / n * seq.
# str * int -> repeated String; list * int -> repeated Vec. Negative
# counts clamp to empty (Python semantics).
def bar(n: int) -> str:
    return "=" * n


def left_mul(n: int) -> str:
    return n * "ab"


def zeros(n: int) -> list[int]:
    return [0] * n


def repeat_pair(n: int) -> list[int]:
    return [1, 2] * n


def clamp_negative() -> str:
    return "x" * -3
