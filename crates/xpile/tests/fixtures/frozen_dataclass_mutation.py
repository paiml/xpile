# PMAT-752 (HUNT-V14 #16 dc-frozen-mutation-not-enforced): a `@dataclass(
# frozen=True)` instance is immutable — Python raises FrozenInstanceError on a
# field assignment. xpile compiled `p.x = 99` and SILENTLY mutated (silent-wrong
# divergence). This is now rejected at transpile time with a clear message.
from dataclasses import dataclass


@dataclass(frozen=True)
class P:
    x: int
    y: int


def mutate() -> int:
    p = P(1, 2)
    p.x = 99  # FrozenInstanceError in Python — must be rejected
    return p.x
