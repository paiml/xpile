from enum import Enum


class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3


def warmth(c: Color) -> int:
    # match on an enum value (terminal form).
    match c:
        case Color.RED:
            return 2
        case Color.GREEN:
            return 1
        case _:
            return 0


def is_primary_pair(c: Color) -> int:
    # `|`-pattern over enum members.
    match c:
        case Color.RED | Color.BLUE:
            return 1
        case _:
            return 0


def label(c: Color) -> int:
    # statement-position match on an enum, assignment bodies.
    n = 0
    match c:
        case Color.RED:
            n = 100
        case Color.GREEN:
            n = 200
        case _:
            n = 300
    return n
