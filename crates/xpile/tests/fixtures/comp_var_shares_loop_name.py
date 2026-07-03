# PMAT-1095 (skeptic pass PMAT-1090, C-F3): Python 3 comprehensions have
# their OWN scope — a comprehension variable sharing an enclosing loop
# variable's name is valid, deterministic CPython, and does NOT leak into
# or clobber the enclosing binding. This shape tripped the PMAT-1085
# same-name-nested-loop refusal (an over-refusal with a factually wrong
# rationale). The list/dict/set-comp list-iterable paths now rename the
# comp variable to a fresh `__forc{N}` exactly like range comprehensions
# have since PMAT-635, so the desugared loop can never collide.
def listcomp_shares_name() -> int:
    total: int = 0
    for x in [1, 2]:
        ys = [x * 2 for x in [3, 4]]
        total = total + x + ys[0] + ys[1]
    return total


def comp_iter_reads_outer() -> int:
    # The comprehension ITERABLE is evaluated in the enclosing scope —
    # `[x, 5]` reads the OUTER x even though the comp rebinds the name.
    total: int = 0
    for x in [1, 2]:
        ys = [x * 2 for x in [x, 5]]
        total = total + x + ys[0] + ys[1]
    return total


def dict_set_comp_share() -> int:
    s: int = 0
    for x in [1, 2]:
        ks = {x: x * 3 for x in [4, 5]}
        zs = {x * 2 for x in [6, 7]}
        s = s + x + ks[4] + len(zs)
    return s
