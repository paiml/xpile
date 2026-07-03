# PMAT-1093 (skeptic pass PMAT-1090, B-F6-mutation-visibility): a bound
# genexp reads its iterable LAZILY in CPython — the xs.append(9) AFTER
# creation is visible at consumption (sum = 28); the eager lowering
# materializes at creation and misses it (14 — SILENT wrong answer).
# Genexp bindings refuse; consume immediately or use an eager list comp.
def entry() -> int:
    xs: list[int] = [1, 4]
    ge = (x * 2 for x in xs)
    xs.append(9)
    return sum(ge)
