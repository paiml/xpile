# PMAT-1043 (sweep #12): two classes with a same-named method, one mutating
# its param and one not, collided in the alias-then-mutate guard's bare-name
# map — the non-mutating sibling MASKED the mutating one, so `a.f(q)` (A.f
# appends to q, q reused after) never fired the guard and the reuse-clone
# silently dropped the append (rust len 1 vs CPython 2). Fix: free-fn and
# method guard maps are separate; same-named methods union their per-position
# mutation flags. This shape now REFUSES loudly (the established
# reused-arg-into-param-mutating posture) instead of miscompiling.
class A:
    def __init__(self) -> None:
        self.n: int = 0

    def f(self, xs: list[int]) -> None:
        xs.append(1)


class B:
    def __init__(self) -> None:
        self.n: int = 0

    def f(self, xs: list[int]) -> int:
        return xs[0]


def main() -> None:
    a: A = A()
    q: list[int] = [5]
    a.f(q)
    print(len(q))
