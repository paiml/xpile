# PMAT-786 (HUNT-V17 #18 DC-OPT-CTOR): a non-None value passed to an
# Optional[T] DATACLASS FIELD in the constructor (Node(5, 10) over next_id:
# Optional[int]) emitted a bare `10i64` against the Option<i64> field slot ->
# rustc E0308. The PMAT-753 coerce-to-optional covered call-args/let-init but
# not struct-literal field values; the ctor lowering now coerces each arg to
# the field's declared type (Some-wrapping a non-None Optional). None and
# keyword args are handled too. Cross-checked vs python3.
from dataclasses import dataclass
from typing import Optional


@dataclass
class Node:
    val: int
    next_id: Optional[int]


def has_next(n: Node) -> int:
    if n.next_id is None:
        return 0
    return 1


def make_pos() -> int:
    n = Node(5, 10)
    return has_next(n)


def make_none() -> int:
    n = Node(7, None)
    return has_next(n)


def make_kw() -> int:
    n = Node(val=3, next_id=8)
    return has_next(n)
