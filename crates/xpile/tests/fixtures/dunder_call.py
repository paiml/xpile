# PMAT-770 (HUNT-V16 DD-06): calling an instance of a class that defines
# `__call__` (`a(x)`) emitted a free `a(x)` call — but `a` is a variable, not a
# function (rustc E0425/E0618: a struct isn't callable). Python's callable
# protocol resolves `a(x)` to `a.__call__(x)`; the call lowering now dispatches
# to that method when the callee is a bound struct-with-__call__ variable.
# Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Adder:
    base: int

    def __call__(self, x: int) -> int:
        return self.base + x


def helper(n: int) -> int:
    return n * 2


def call_instance() -> int:
    a = Adder(10)
    return a(5)


def plain_call() -> int:
    # a real function call is unaffected
    return helper(7)
