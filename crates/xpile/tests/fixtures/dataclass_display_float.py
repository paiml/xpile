# PMAT-840 (HUNT-V26 #9): a dataclass with a float field had no generated Display
# impl (display_eligible was int/bool-only) → str()/f-string of it was rustc
# E0277. A float field formats the same in the dataclass repr as str(float) does;
# Display now covers int/bool/FLOAT dataclasses (a str field, which needs Python's
# quoted-escaped repr, stays deferred). Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Pt:
    x: float
    y: float
    n: int


@dataclass
class IntBool:
    a: int
    b: bool


def pt_repr() -> str:
    return str(Pt(1.5, 2.0, 3))


def pt_whole() -> str:
    return str(Pt(4.0, 0.0, 1))


def pt_fstring() -> str:
    return f"here: {Pt(1.5, 2.5, 9)}"


def intbool_repr() -> str:
    return str(IntBool(5, True))
