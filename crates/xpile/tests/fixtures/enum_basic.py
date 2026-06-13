from enum import Enum


class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3


class Signal(Enum):
    STOP = 0
    GO = 1


def red_value() -> int:
    # `C.NAME.value` → the discriminant literal.
    return Color.RED.value


def blue_value() -> int:
    return Color.BLUE.value


def green_name() -> str:
    # PMAT-515: `C.NAME.name` → the variant name string (compile-time known).
    return Color.GREEN.name


def is_go(s: Signal) -> bool:
    # Enum-typed param + equality against a member.
    return s == Signal.GO


def passthrough() -> int:
    # Enum-typed local; compare two members.
    c = Color.GREEN
    if c == Color.GREEN:
        return 10
    return 0
