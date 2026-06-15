# PMAT-688: a walrus assignment `(t := E)` in an `if` condition (which evaluates
# once) is hoisted to `let mut t = E;` before the if (Python leaks t to the
# enclosing scope). Was an opaque `Discriminant(1)` reject. Restricted to
# unconditional positions (single compare / arithmetic) — `and`/`or` short-circuit
# walruses still reject cleanly.
def f(total: int) -> int:
    if (t := total + 1) > 10:
        return t
    return 0


def bare(total: int) -> int:
    if (t := total):
        return t * 2
    return -1


def arith(a: int) -> int:
    if (x := a) + 5 > 8:
        return x
    return 0
