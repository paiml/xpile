# PMAT-1092 (skeptic pass PMAT-1090, A-F4): a name FIRST-bound inside the
# `try` body (and handler) and read after the `try`. Valid CPython (prints 3),
# but the emitted arms are block-scoped catch_unwind blocks, so the bindings
# don't survive — this was rustc E0425 far from the cause. Now refused at
# lowering with the scoping truth + the bind-before-the-try workaround.
def after_try() -> int:
    try:
        x = 1
        y = 2
    except ValueError:
        x = -1
        y = -2
    return x + y
