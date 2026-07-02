# PMAT-1034: a FRESH loop var (never bound before the `for`) read
# UNCONDITIONALLY after the loop is Python's UnboundLocalError trap when the
# iterable is empty — `for x in range(n)` with `n <= 0` never binds `x`, so
# `return x` raises. On runtime-abort targets (Rust `panic!` / WASM
# `unreachable` trap) xpile emits a guard that fires exactly where CPython
# raises; on lanes with no portable abort (WGSL/PTX/SPIR-V/Lean/shell) NO guard
# is emitted — refusing the shape would block a program those lanes execute
# exactly on every non-empty input. A `range(n)` loop keeps the whole thing in
# the scalar/control subset, so WGSL transpiles it too (the ONLY lane
# difference is the guard). Cross-checked vs python3.


def last_i(n: int) -> int:
    for x in range(n):
        pass
    return x
