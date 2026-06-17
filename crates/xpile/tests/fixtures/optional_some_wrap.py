# PMAT-753 (HUNT-V15 ONF-1/ONF-2): a concrete (non-None) value passed to an
# `Optional[T]` parameter, or assigned to an `Optional[T]`-annotated local, was
# emitted as a bare `T` against a Rust `Option<T>` slot → rustc E0308 (rustc
# itself suggests `Some(..)`). `None` already lowered to `None` correctly. The
# value is now wrapped in `Some(...)` at the call site (via the callee's
# declared param types) and at an annotated-local initializer. An already-
# Optional value passes through (no `Option<Option<T>>`). Cross-checked vs
# python3.
from typing import Optional


def f(x: Optional[int]) -> int:
    if x is None:
        return 0
    return x


def via_lit() -> int:
    # call an Optional[int] param with a concrete literal → f(Some(5))
    return f(5)


def via_none() -> int:
    return f(None)


def local_init() -> int:
    # annotated Optional local initialized with a concrete value → Some(5)
    y: Optional[int] = 5
    if y is None:
        return -1
    return y
