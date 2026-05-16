# While-loop + mutable rebinding fixture (PMAT-006).
#
# `sum_to(n)` returns 1 + 2 + ... + n via an iterative accumulator.
# Exercises:
#   - first assign to `total` and `i` → Stmt::Let { mutable: true, ... }
#   - while-loop body containing reassignments → Stmt::Assign
#   - same name (`total`, `i`) reused across iterations
def sum_to(n: int) -> int:
    total = 0
    i = 1
    while i <= n:
        total = total + i
        i = i + 1
    return total
