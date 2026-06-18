# PMAT-776 (HUNT-V17 #2): a dataclass with a custom __str__ rendered as the
# hard-coded PMAT-760 field-repr (`ClassName(f=v)`) in f-strings/print, with the
# __str__ method dead — silently wrong. The generated Display now delegates to
# __str__ when present, for ANY field types (so a str-building __str__ also
# enables f-string rendering of an otherwise-ineligible struct). Cross-checked
# vs python3. (str(obj) — the builtin over a struct — is a separate follow-up.)
from dataclasses import dataclass


@dataclass
class C:
    v: int

    def __str__(self) -> str:
        return "XYZ"


@dataclass
class Pt:
    x: int
    y: int

    def __str__(self) -> str:
        return "(" + str(self.x) + "," + str(self.y) + ")"


def fstr_c() -> str:
    return f"val={C(5)}"


def fstr_pt() -> str:
    return f"pt={Pt(3, 4)}"
