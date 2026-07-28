# PMAT-124 / PMAT-1432 — shape demo (NOT a witness) for the Python list → Rust Vec translation.
#
# xpile-runtime-vote: none.
#
# NO test loads this file, so it casts NO Runtime-stratum vote
# (PMAT-1432). Until PMAT-1432 it did, because the fixture pass of
# `xpile quorum` counted any file that merely CONTAINED a contract
# ID. Wiring a test that loads this file by name is what turns it
# into evidence; on its own it documents a shape, nothing more.
#
# Shape documented, for:
#   C-XLATE-PY-LIST-TO-VEC
#
# Small Python module exercising list literals, list comprehension
# (when supported), and list-based iteration shapes that the
# translation contract pins down. depyler-frontend's general list
# handling is post-v0.1.0 (the v0.1.0 list support is scoped to
# subprocess.run([...]) recognition for the cross-domain shell
# path); the fixture is in place ahead of that wiring so the
# contract can move from 3-stratum QUORUM to full 4-stratum.
#
# A dedicated round-trip test verifying that list operations
# preserve order and length per `iteration_order_preserved`
# (Lean theorem PMAT-060 / Kani harness PMAT-061) is
# XPILE-XLATE-LIST-RUNTIME-001 future work.


def first_three() -> int:
    xs: list[int] = [10, 20, 30]
    # Length preservation is part of the contract's invariants
    # set. Once depyler-frontend grows general list lowering, the
    # transpiled Rust will use Vec<i64>.
    return xs[0] + xs[1] + xs[2]
