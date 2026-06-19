# PMAT-830 (HUNT-V25 #6): str(<dataclass>) mis-inferred as I64 — a function
# returning it ("body produces I64") was rejected. The dataclass has a Display
# impl (the field-repr), which an f-string field f"{p}" already uses; str(p) now
# routes through the same Display path. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class P:
    x: int
    y: int


def bare() -> str:
    return str(P(3, 4))


def in_concat() -> str:
    return "pt=" + str(P(1, 2))
