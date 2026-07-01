# PMAT-1019 (sweep #9): TERNARY returns launder too — `return xs if x else [0]`
# aliases through its arm exactly like the statement-if form (which was
# caught); the conditional-expression form silently cloned (4 confirmed
# findings incl. recursion-shaped). may_return_param now scans IfExp arms.
# Nested chains (`ident(ident(c))`) are likewise collected recursively at the
# call site.
def keep(xs: list[int], flag: bool) -> list[int]:
    return xs if flag else [0]


def main() -> int:
    a: list[int] = [1, 2]
    d = keep(a, True)
    d.append(9)
    return len(a)
