# PMAT-867 (HUNT-V31 #3): a chained comparison `a OP b OP c` must evaluate its
# operands strictly left-to-right (Python). The right-nested lowering bound the
# MIDDLE operand (`let __t1 = b`) before inlining the LEFT into the `if`
# condition, so `b` evaluated before `a` — wrong stdout order, and a wrong
# boolean under shared mutable state. The first operand is now bound to `__t0`
# before `__t1`. Cross-checked vs python3 (boolean results).


def in_range(x: int) -> bool:
    return 1 <= x <= 10


def strict(x: int) -> bool:
    return 3 < x < 5


def triple(a: int, b: int, c: int) -> bool:
    return a < b < c
