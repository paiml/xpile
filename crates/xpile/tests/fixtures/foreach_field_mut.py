# PMAT-828 (HUNT-V25 #5): a dataclass field mutation inside a for-loop
# (for p in pts: p.x = ...) emitted `for p in pts.iter().cloned()` with p not mut
# → rustc E0594, and the clone discarded the mutation (silent-wrong). The
# in-place-mutation detection (PMAT-816) now covers attribute assignment and a
# Struct element type, and the AST mut-inference marks a LOCAL iterable mut so
# `pts.iter_mut()` borrows it. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class P:
    x: int


def scale_and_sum() -> int:
    pts = [P(1), P(2), P(3)]
    for p in pts:
        p.x = p.x * 10
    total = 0
    for p in pts:
        total += p.x
    return total
