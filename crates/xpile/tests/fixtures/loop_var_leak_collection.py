# PMAT-838 (HUNT-V26 #1): Python leaks a for-loop variable; reading it after the
# loop is the last-iterated value. A range loop already leaks (while-rewrite); a
# COLLECTION loop over a FRESH var kept the body-scoped `for x` binding → a
# post-loop read was rustc E0425. A fresh, primitive-element loop var read after
# the loop is now pre-declared `let mut` + assigned each iteration. A loop var
# NOT read after stays native; a pre-bound var keeps the existing leak path.
# Cross-checked vs python3. (PMAT-784's loop_var_leak.py covers the pre-bound case.)


def leaked_int(xs: list[int]) -> int:
    for x in xs:
        pass
    return x


def leaked_str(words: list[str]) -> str:
    for w in words:
        pass
    return w


def not_read_after(xs: list[int]) -> int:
    total = 0
    for x in xs:
        total += x
    return total


def find_prebound(xs: list[int]) -> int:
    x = -1
    for x in xs:
        if x > 10:
            break
    return x


def leaked_char(s: str) -> str:
    for ch in s:
        pass
    return ch
