# PMAT-1104 (case a): a name bound at MULTIPLE nesting levels of a for-target —
# `for (x, y), x in …` — Python binds left-to-right so the LAST occurrence wins
# (x = the top-level 3, not the nested 1). xpile deferred the nested destructure
# AFTER the header binding → first-wins → SILENT wrong output. Fix: on a
# cross-nesting dup, bind every element in SOURCE order.
def last_wins() -> int:
    rows = [((1, 2), 3), ((4, 5), 6)]
    total: int = 0
    for (x, y), x in rows:
        total = total + x * 100 + y
    return total
