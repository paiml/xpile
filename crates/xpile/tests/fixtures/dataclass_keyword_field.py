# PMAT-810 (HUNT-V21 #1): a dataclass field named after a Rust keyword
# (type/match/ref/move/...) emitted unescaped at every site — `pub type: i64`,
# `Event { type: .. }`, `(e).type`, `self.type` — a rustc keyword parse error.
# The reserved-ident escape pass now escapes struct field names + field accesses
# + struct-literal keys to the `r#` raw form; the Display field-repr LABEL strips
# the `r#` back so the repr shows the Python name. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Event:
    type: int
    match: int


def field_access() -> int:
    e = Event(5, 7)
    return e.type + e.match


def repr_label() -> str:
    return f"{Event(3, 4)}"
