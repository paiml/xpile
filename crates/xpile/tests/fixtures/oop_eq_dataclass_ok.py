# PMAT-1165: a @dataclass gets Python's auto-generated STRUCTURAL __eq__, which
# MATCHES the derived Rust `PartialEq` — so `==`/`!=` between dataclass instances
# still lowers and agrees with CPython. Only PLAIN-class identity `==` is refused;
# the dataclass keep-working case must NOT be caught by the PMAT-1165 refusal.
from dataclasses import dataclass


@dataclass
class P:
    x: int


def eq() -> bool:
    return P(5) == P(5)


def ne() -> bool:
    return P(5) != P(6)
