# PMAT-1011 (sweep #7): bool↔float comparison — the "separate rarer follow-up"
# deferred by PMAT-617. Python's bool is an int subtype, so `True == 1.0` /
# `True < 1.5` / `0.5 < True` are valid; xpile emitted a bare `bool OP f64`
# (rustc E0308). The bool side coerces to i64 and routes through the exact
# MixedIntFloatCmp template (same as int↔float, PMAT-745).
def eq_one(b: bool) -> bool:
    return b == 1.0


def lt_threshold(b: bool, t: float) -> bool:
    return b < t


def rev_cmp(x: float, b: bool) -> bool:
    return x < b
