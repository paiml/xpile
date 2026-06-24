def both(a: bool, b: bool) -> bool:
    return a and b


def either(a: bool, b: bool) -> bool:
    return a or b


def negate(a: bool) -> bool:
    return not a


def mixed(a: bool, b: bool, c: bool) -> bool:
    # operator polarity preserved across nesting: (a and not b) or c
    return (a and not b) or c
