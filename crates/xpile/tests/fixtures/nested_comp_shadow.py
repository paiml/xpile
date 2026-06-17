# PMAT-754 (HUNT-V15 #1 COMP-SHADOW-RANGE-LIST): a nested comprehension whose
# inner loop-var SHADOWS an enclosing comprehension var of the same name
# resolved to the leaked outer `__forcN` while-counter instead of the inner
# binding. `[[x for x in range(5)] for x in range(2)]` made every inner row
# `[counter; 5]` → [[0,0,0,0,0],[1,1,1,1,1]] (sum 5) instead of Python's
# [[0,1,2,3,4],[0,1,2,3,4]] (sum 20) — silent-wrong, and rustc warned
# `unused variable: x`. The fix clears the active rename when an inner comp
# re-binds the shadowed name. Cross-checked vs python3.


def nested_sum() -> int:
    ys = [[x for x in range(5)] for x in range(2)]
    s = 0
    for row in ys:
        for v in row:
            s = s + v
    return s  # 20


def distinct_names() -> int:
    # different inner/outer names — unaffected
    ys = [[y for y in range(3)] for x in range(2)]
    s = 0
    for row in ys:
        for v in row:
            s = s + v
    return s  # 6


def shadow_over_list() -> int:
    # inner comp iterates a LIST while shadowing the outer range var
    base = [10, 20, 30]
    ys = [[x for x in base] for x in range(2)]
    return ys[0][1] + ys[1][2]  # 50
