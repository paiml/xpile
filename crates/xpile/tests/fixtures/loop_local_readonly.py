# PMAT-466 regression (review #1): an annotated local declared inside a
# loop body but never mutated must NOT be emitted `let mut` — clippy
# `-D warnings` (the project's pre-push gate) rejects `unused_mut`. The
# mutability pre-pass must count a binding declaration once, not double
# it for being inside a loop (each iteration re-binds a fresh local).
def f(xs: list[int]) -> int:
    total: int = 0
    for x in xs:
        tmp: int = x
        total = total + tmp
    return total
