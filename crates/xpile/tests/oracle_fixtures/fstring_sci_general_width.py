# PMAT-969 (correctness-hunt): the WIDTH/ALIGN-combined scientific (`:12.2e`) and
# general (`:14g`) float specs. The `e`/`g` value is rendered to its Python string
# first (the existing FloatSciStr / FloatGeneralStr lowering), then padded by an
# optional `[fill][align][width]` prefix routed through a `FormatSpec` — no new IR
# node. A bare width defaults to RIGHT-align (a numeric value); the explicit
# `0`-fill form works via the fill+align prefix; Rust's string fill/align is
# char-count-based and shares Python's even-pad center split, and an `e`/`g` render
# is pure ASCII, so it reproduces CPython byte-for-byte. This oracle fixture pins
# the supported surface byte-identical to CPython.


def main() -> None:
    x: float = 1234.5
    big: float = 123456789.0
    neg: float = -1234.5
    tiny: float = 0.000123
    # scientific, width-combined.
    print(f"[{x:12.2e}]")        # right-align default
    print(f"[{neg:12.2e}]")
    print(f"[{x:>12.2e}]")
    print(f"[{x:<12.2e}]")
    print(f"[{x:^14.3e}]")       # center even-pad split
    print(f"[{x:*>14.2e}]")      # explicit fill
    print(f"[{x:0>12.2e}]")      # explicit 0-fill
    print(f"[{neg:0>12.2e}]")
    print(f"[{5:12.2e}]")        # int coerced to float
    # general, width-combined.
    print(f"[{x:14g}]")
    print(f"[{big:14g}]")
    print(f"[{x:>14g}]")
    print(f"[{x:<14g}]")
    print(f"[{x:^16.3g}]")
    print(f"[{1234567:14g}]")    # int coerced → scientific %g
    # labeled / multi-field composition.
    print(f"hi [{tiny:>10.1e}] bye")
    print(f"{x:>12.2e}{big:<14g}")
