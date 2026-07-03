# PMAT-1166 (counterpart): the container refusal must NOT be too broad. A
# @dataclass gets Python's auto-generated STRUCTURAL __eq__, which matches the
# derived Rust `PartialEq`, so container `==`, tuple `==`, and membership `in`
# over dataclass instances still lower AND agree with CPython. Primitive-element
# containers (`[1, 2] == [1, 2]`, `3 in [1, 2, 3]`) are likewise unaffected.
from dataclasses import dataclass


@dataclass
class P:
    x: int


def list_eq() -> bool:
    return [P(1), P(2)] == [P(1), P(2)]


def tuple_eq() -> bool:
    return (P(1),) == (P(1),)


def membership() -> bool:
    return P(1) in [P(1), P(2)]


def primitive_list_eq() -> bool:
    return [1, 2] == [1, 2]


def primitive_membership() -> bool:
    return 3 in [1, 2, 3]
