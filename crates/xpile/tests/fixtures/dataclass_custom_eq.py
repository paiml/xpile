# PMAT-762 (HUNT-V16 DD-01): a dataclass with a custom __eq__ got
# `#[derive(PartialEq)]` AND a dead __eq__ method, so `==` dispatched to the
# structural derive (all fields) — silently overriding the user's semantics.
# The structural derive is now suppressed and `==` delegates to __eq__ via a
# generated `impl PartialEq`. This also makes `x in list` use the right equality
# (DD-02). Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Pt:
    x: int
    y: int

    def __eq__(self, other: "Pt") -> bool:
        return self.x == other.x


def cmp_eq() -> int:
    # (1,2) == (1,9) is True under custom __eq__ (only .x compared)
    if Pt(1, 2) == Pt(1, 9):
        return 1
    return 0


def cmp_ne() -> int:
    if Pt(1, 2) == Pt(2, 2):
        return 1
    return 0


def in_list() -> int:
    # `x in list` (Vec::contains) must use the custom equality
    xs = [Pt(1, 5), Pt(3, 7)]
    if Pt(1, 99) in xs:
        return 1
    return 0
