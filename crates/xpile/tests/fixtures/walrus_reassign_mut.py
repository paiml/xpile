# PMAT-809 (HUNT-V22 CA-1): a walrus-bound name reassigned once emitted an
# immutable `let` → rustc E0384. The walrus lives in the if-condition expression
# (not a Stmt::Assign), so the mut-inference walk_counts missed its binding —
# `n = 99` later looked like the first/only write. walk_counts now counts walrus
# targets in the condition, so a reassigned walrus binding becomes `let mut` (and
# a never-reassigned one stays plain `let`). Cross-checked vs python3.


def reassign() -> int:
    if (n := 10) > 5:
        n = 99
    return n


def no_reassign() -> int:
    if (m := 7) > 3:
        return m
    return 0


def reassign_in_else() -> int:
    if (k := 2) > 5:
        return k
    else:
        k = 50
    return k
