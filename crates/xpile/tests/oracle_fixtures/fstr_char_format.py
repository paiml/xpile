# PMAT-945 (correctness-hunt): the `:c` char-format spec converts an INTEGER to
# the single character at that Unicode code point — `f"{65:c}"` == "A". This is
# exactly Python's `chr(n)` (CPython's `int.__format__` for code `c` delegates to
# chr), and xpile now routes it to the existing `Expr::Chr` lowering (no new IR
# node). This oracle fixture pins the supported surface byte-identical to CPython:
# literals, a variable, hex code points, multi-byte (€) and astral (😀) scalars,
# the shared `format(n, 'c')` builtin, a bool delegating to int, and multi-field
# composition. The control-char / out-of-range edges live in the e2e test.


def main() -> None:
    print(f"{65:c}")        # A
    print(f"{97:c}")        # a
    print(f"{90:c}")        # Z
    print(f"{8364:c}")      # €  (3-byte UTF-8)
    print(f"{0x1F600:c}")   # 😀 (astral / 4-byte UTF-8)
    print(format(66, "c"))  # B  (the shared format() builtin path)
    code = 0x2764
    print(f"{code:c}")      # ❤  (a variable operand)
    print(f"{True:c}")      # \x01 (bool delegates to int -> chr(1))
    print(f"go {72:c}{73:c}!")  # go HI!  (multi-field composition)
    n = 0x4E2D
    print(f"han: {n:c}")    # han: 中
