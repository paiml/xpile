# PMAT-1016A: mutating `&mut self` methods — the read-only-first-cut refusal
# lifted. A method that assigns `self.field` (or, via the transitive AST
# fixpoint, calls another mutating method on `self`) lowers with a `&mut self`
# receiver; the caller's receiver binds `let mut c` (the mutability pre-walk
# bumps receivers of registered mutating methods); statement-position calls
# (`c.bump()`) lower via the new Stmt::SideEffectCall. Frozen dataclasses and
# explicit `__init__` still refuse (FrozenInstanceError / slice B).
from dataclasses import dataclass


@dataclass
class Counter:
    count: int

    def bump(self) -> None:
        self.count = self.count + 1

    def double_bump(self) -> None:
        self.bump()
        self.bump()

    def value(self) -> int:
        return self.count


def run() -> int:
    c = Counter(0)
    c.bump()
    c.double_bump()
    return c.value()
