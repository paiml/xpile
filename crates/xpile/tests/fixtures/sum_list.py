# PMAT-458 / v0.2.0 Track 1.B: for-each iteration over list[int].
# Exercises Stmt::ForEach + a mutable accumulator + C-PY-INT-ARITH's
# checked-add discharge inside the loop body. Closes the spec §23
# ⏳ entry "`for` over non-range iterables".
def total(xs: list[int]) -> int:
    s = 0
    for x in xs:
        s = s + x
    return s
