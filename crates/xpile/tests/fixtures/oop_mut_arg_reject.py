# PMAT-1016A guard (the NON-NEGOTIABLE slice-A criterion): passing a struct
# to a function that calls a MUTATING method on the parameter, with the caller
# re-reading the struct afterwards, would silently drop the mutation (the
# PMAT-588 ownership pre-pass clones the re-read arg; Python's reference
# semantics make the caller see count == 1, the clone-emit prints 0). The
# PMAT-884 alias-then-mutate clean-reject now covers Stmt::SideEffectCall
# statements (the guard's expression walker previously never visited them —
# the slice-A adversarial differential caught the DIVERGE rust=0/cpython=1).
from dataclasses import dataclass


@dataclass
class Counter:
    count: int

    def bump(self) -> None:
        self.count = self.count + 1

    def value(self) -> int:
        return self.count


def mutate_it(c: Counter) -> None:
    c.bump()


def main() -> int:
    c = Counter(0)
    mutate_it(c)
    return c.value()
