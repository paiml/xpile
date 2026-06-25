# PMAT-944 (correctness-hunt): Python `and`/`or` are VALUE-returning short-circuit
# operators, not boolean operators — `0 or "d"` is `"d"`, `5 and "y"` is `"y"`.
# A differential sweep found xpile silently folded a MIXED / non-bool `and`/`or`
# to a Rust `bool` in VALUE position (`print(0 or "d")` printed `True`, not `"d"`).
# The fix REJECTS that value-position union (no single Rust type) — see the e2e
# test `boolop_value_union_rejected`. This oracle fixture pins the cases that DO
# transpile and must stay byte-identical to CPython:
#   (1) same-type operands return the operand value (the PMAT-637/638 fold), and
#   (2) mixed / non-bool `and`/`or` in a BOOLEAN context (if / while / not /
#       bool() / assert / ternary cond) folds to truthiness, which is correct.


def main() -> None:
    # (1) same-type value-return — must return the OPERAND, not a bool
    a = 0
    print(a or 10)          # 10  (a falsy -> second)
    b = 7
    print(b or 10)          # 7   (b truthy -> first)
    s = ""
    print(s or "default")   # default
    t = "hi"
    print(t or "default")   # hi
    print(b and 99)         # 99  (b truthy -> second)
    print(a and 99)         # 0   (a falsy -> first)
    x = 0.0
    print(x or 2.5)         # 2.5
    # chain: returns the first truthy / the last
    c = 0
    d = 0
    print(c or d or 9)      # 9

    # (2) mixed / non-bool `and`/`or` in a BOOLEAN context — truthiness is correct
    y = 0
    if y or "d":            # "d" truthy -> enter
        print("if-or-taken")
    xs = [3]
    if xs and xs[0]:        # both truthy -> enter
        print("if-and-taken")
    print(not (y or "d"))   # not "d" -> False
    print(bool(y or "d"))   # True
    print("yes" if (xs and xs[0] > 1) else "no")  # 3>1 -> yes
    n = 0
    while y or n < 2:       # y falsy, n<2 -> loop until n==2
        n = n + 1
    print(n)                # 2

    # all-bool `and`/`or` in value position stays a bool (Python: bool-equal)
    p = True
    q = False
    print(p or q)           # True
    print(p and q)          # False
