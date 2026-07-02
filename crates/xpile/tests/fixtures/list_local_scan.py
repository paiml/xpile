# PMAT-1033: list[int] LOCALS + list LITERALS on the WASM lane — the
# sweep-#11 a-series scan/filter/index-write family, previously executable
# only on the Rust lane (the WASM map_type List gate refused every local).
# Both lanes must execute this == CPython 16:
#   total = 3 + 7 + 2 = 12 (11 skipped by the continue)
#   xs[0] = 12; xs[0] + len(xs) = 12 + 4 = 16
def run() -> int:
    xs = [3, 7, 11, 2]
    total = 0
    for x in xs:
        if x > 10:
            continue
        total = total + x
    xs[0] = total
    return xs[0] + len(xs)
