# PMAT-947 (correctness-hunt): a bare precision `.N` on a STR f-string field
# truncates to N chars. Rust's `{:.N}` over a `String` is the IDENTICAL char-count
# truncation (multibyte/astral counted by char, not byte), and the precision is a
# MAX (a precision >= len is a no-op). xpile routes it through the existing
# FormatSpec node (of_float=false so the float NaN-guard is correctly skipped).
# This oracle fixture pins the supported surface byte-identical to CPython: a plain
# truncation, `.0` (empty), a precision wider than the string, multi-byte (é) and
# astral (😀) truncation by char count, the shared str.format() path, and
# multi-field composition.


def main() -> None:
    s = "hello"
    print(f"[{s:.3}]")              # [hel]
    print(f"[{s:.0}]")             # []      (.0 -> empty)
    print(f"[{s:.10}]")            # [hello] (precision >= len is a no-op max)
    print(f"[{'world':.2}]")       # [wo]    (a literal operand)
    m = "héllo"
    print(f"[{m:.3}]")             # [hél]   (multi-byte, truncated by CHAR count)
    e = "😀😀😀😀"
    print(f"[{e:.2}]")             # [😀😀]  (astral / 4-byte, truncated by char)
    z = "中文字"
    print(f"[{z:.2}]")             # [中文]
    print("{:.4}".format("formatting"))  # form   (the str.format() path)
    a = "abcd"
    b = "wxyz"
    print(f"[{a:.2}|{b:.3}]")      # [ab|wxy] (multi-field composition)
