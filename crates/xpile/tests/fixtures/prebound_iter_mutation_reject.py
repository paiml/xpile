# PMAT-1038 guard (the h6/h10 witness): a PRE-BOUND loop target whose body
# mutates the element in place — the value model cannot both propagate the
# mutation (iter_mut) and leak the last element into the outer name; the
# leak clone silently absorbed the append (grid rows unchanged: rust panic /
# wrong value vs CPython 9). Pre-existing for genuinely pre-bound names; the
# PMAT-1038 hoist makes builder locals pre-bound BY DESIGN, so the refusal
# keeps the shape loud. Use a fresh loop-var name instead.
def main() -> None:
    grid = [[1], [2]]
    row = [0]
    for row in grid:
        row.append(9)
    print(grid[0][1])
