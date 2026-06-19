# PMAT-841 (HUNT-V26 #9): a dataclass with a str field now gets a generated Display
# too — Python's dataclass repr quotes str fields (P(name='hi')), switching to
# double quotes when the value contains a single quote. The str field reuses the
# repr(str) escaping (ReprStr). Completes int/bool/float/str dataclass repr.
# Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class P:
    name: str
    age: int
    score: float


def mixed_repr() -> str:
    return str(P("Ann", 30, 1.5))


def mixed_fstring() -> str:
    return f"<{P('Bo', 7, 2.0)}>"


def embedded_quote() -> str:
    return str(P("a'b", 1, 0.5))
