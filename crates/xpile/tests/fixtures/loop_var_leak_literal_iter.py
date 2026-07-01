# PMAT-1012 (sweep #7): Python LEAKS the for-loop variable into the enclosing
# scope — after `for ch in "wxyz": pass`, `ch` is "z". The PMAT-838 pre-declare
# (which synthesizes `let mut ch = <default>` when a FRESH loop var is read
# after the loop) matched only a plain-NAME iterable, so a str/list LITERAL or
# a slice iterable never pre-declared → the post-loop read was rustc E0425.
# Now the iterable's type is derived by probe-lowering the iter expression.
def last_char() -> str:
    for ch in "wxyz":
        pass
    return ch


def last_elem() -> int:
    for x in [7, 8, 9]:
        pass
    return x


def last_tail(xs: list[int]) -> int:
    for x in xs[1:]:
        pass
    return x
