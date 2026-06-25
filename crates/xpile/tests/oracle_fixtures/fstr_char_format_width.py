# PMAT-946 (correctness-hunt): the WIDTH/ALIGN-combined `:c` char-format spec.
# `c` is an INTEGER presentation type, so a bare width defaults to RIGHT-align; a
# fill+align prefix and the explicit `0`-fill form work verbatim (Rust's string
# fill/align is char-count-based and matches Python's `len`-based padding and
# center even-pad split). xpile routes `Expr::Chr` (a single-char String) through
# a `FormatSpec` carrying the `[fill][align][width]` prefix — no new IR node. This
# oracle fixture pins the supported surface byte-identical to CPython: bare width,
# left/right/center align, a fill char, an explicit 0-fill, multi-byte (€) and
# astral (😀) scalars, a bool delegating to int, the shared `format(n, ...)`
# builtin, and multi-field composition.


def main() -> None:
    print(f"[{65:>5c}]")        # [    A]
    print(f"[{65:<5c}]")        # [A    ]
    print(f"[{67:^7c}]")        # [   C   ]
    print(f"[{65:5c}]")         # [    A]  (bare width → right-align)
    print(f"[{65:*>5c}]")       # [****A]
    print(f"[{66:*<5c}]")       # [B****]
    print(f"[{67:.^7c}]")       # [...C...]
    print(f"[{65:0>5c}]")       # [0000A]  (explicit 0-fill)
    print(f"[{8364:>4c}]")      # [   €]   (3-byte UTF-8, padded by char count)
    print(f"[{0x1F600:>3c}]")   # [  😀]   (astral / 4-byte, padded by char count)
    print(f"[{0x4E2D:>5c}]")    # [    中]
    print(f"[{True:>3c}]")      # [  \x01] (bool delegates to int → chr(1))
    print(format(72, ">4c"))    #    H    (the shared format() builtin path)
    code = 0x2764
    print(f"[{code:^5c}]")      # [  ❤  ]  (a variable operand, center)
    print(f"go {72:>3c}{73:<3c}!")   # go   HI  !  (multi-field composition)
