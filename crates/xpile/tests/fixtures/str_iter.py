# PMAT-502cl (Tranche 2): string iteration `for c in s` — each char a 1-char
# string. Lowers to a ForEach over the string's chars (Expr::StrChars).
def count_vowels(s: str) -> int:
    n = 0
    for c in s:
        if c == "a" or c == "e" or c == "i" or c == "o" or c == "u":
            n = n + 1
    return n


def reverse_str(s: str) -> str:
    out = ""
    for c in s:
        out = c + out
    return out
