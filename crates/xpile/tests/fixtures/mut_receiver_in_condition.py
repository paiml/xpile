def drain_param(xs: list[int]) -> int:
    # `xs.pop()` in a `while` condition mutates `xs` — the receiver must be
    # emitted `mut` (was rustc E0596: cannot borrow `xs` as mutable).
    count = 0
    while len(xs) > 0 and xs.pop() >= 0:
        count = count + 1
    return count


def drain_local() -> int:
    # Same, but the popped receiver is a *local*, not a param.
    ys: list[int] = [3, 2, 1, 0]
    total = 0
    while len(ys) > 0 and ys.pop() > 0:
        total = total + 1
    return total


def check_if(zs: list[int]) -> str:
    # `.pop()` in an `if` condition.
    if zs.pop() == 9:
        return "nine"
    return "other"


def assert_pop(ws: list[int]) -> int:
    # `.pop()` in an `assert` condition.
    assert ws.pop() >= 0
    return len(ws)
