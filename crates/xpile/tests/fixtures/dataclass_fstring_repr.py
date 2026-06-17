# PMAT-760 (HUNT-V15 #6): a dataclass instance in an f-string emitted
# `format!("{}", obj)` but the struct only derives Debug → rustc E0277. Python's
# dataclass __repr__ is `ClassName(f1=v1, …)`. The backend now generates a
# matching `Display` for an all-int/bool dataclass (where Rust `{}` / a
# True/False map equals Python repr), so it renders in an f-string. A dataclass
# with str/float/nested fields is rejected cleanly (deferred). Cross-checked vs
# python3.
from dataclasses import dataclass


@dataclass
class P:
    x: int
    y: int


@dataclass
class Flags:
    a: bool
    n: int


def lone() -> str:
    return f"{P(1, 2)}"


def multi() -> str:
    return f"pt={P(3, 4)}"


def with_bool() -> str:
    return f"flags={Flags(True, 5)}"
