# PMAT-777 (HUNT-V17 #3): a custom __ne__ was never dispatched — the PMAT-762
# `impl PartialEq` only set `fn eq`, so `!=` used Rust's default `!eq()` and the
# user __ne__ was dead code (silent-wrong). Both backends now emit `fn ne`
# delegating to __ne__; when __ne__ is defined WITHOUT __eq__, the structural
# `==` (dataclass all-fields) is emitted by hand. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class C:
    v: int

    def __eq__(self, other: "C") -> bool:
        return self.v == other.v

    def __ne__(self, other: "C") -> bool:
        return False


@dataclass
class D:
    a: int
    b: int

    def __ne__(self, other: "D") -> bool:
        return self.a != other.a


def ne_always_false() -> int:
    # __ne__ returns False → `!=` is False
    return 111 if C(1) != C(2) else 222


def ne_only_struct_eq() -> int:
    # D has no __eq__ → structural == (a AND b): (1,9)==(1,5) is False
    return 1 if D(1, 9) == D(1, 5) else 0


def ne_only_custom_ne() -> int:
    # custom __ne__ compares only .a: (1,9) != (1,5) is (1 != 1) = False
    return 1 if D(1, 9) != D(1, 5) else 0
