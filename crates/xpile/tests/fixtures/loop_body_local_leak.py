# PMAT-1038: loop-scope name-model mismatches — Python leaks BODY-BOUND loop
# locals to function scope; the emitted Rust `let` died at the loop block
# (rustc E0425 on reuse, e.g. `row = [i, i]` in a builder loop then
# `for row in grid:`). Fresh top-level body bindings read after the loop are
# now PRE-DECLARED `let mut <name>: T = <default>` (RHS probe-typed with the
# for-target registered; range targets are trivially int) — the body binding
# lowers as a reassignment. Same PMAT-838/1015 empty-iterable tradeoff.
# Also here: the alias analysis no longer edges DEFINITELY-SCALAR embeds
# (`row = [i, i * 2]` falsely refused "aliases `i` and `row`" — ints are
# Python value copies), and the pre-bound-target + element-mutation combo
# refuses precisely (see the reject twin: the leak clone silently absorbed
# the mutation). Differentially verified vs CPython (MATCH 1/ccc!ccc!/15/6).
def rebuilt_rows() -> int:
    grid: list[list[int]] = []
    for i in range(2):
        row = [i, i]
        grid.append(row)
    total = 0
    for row in grid:
        total = total + row[0]
    return total


def leak_after_collection_loop() -> str:
    words = ["a", "bb", "ccc"]
    best = ""
    for w in words:
        cand = w + "!"
        if len(cand) > len(best):
            best = cand
    return best + cand


def leak_after_while() -> int:
    n = 3
    total = 0
    while n > 0:
        sq = n * n
        total = total + sq
        n = n - 1
    return total + sq


def scalar_embed_matrix() -> int:
    grid: list[list[int]] = []
    for i in range(3):
        row = [i, i * 2]
        grid.append(row)
    total = 0
    for r in grid:
        total = total + r[1]
    return total

