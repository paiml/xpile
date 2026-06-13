def show_bool(b: bool) -> str:
    return "%s" % b


def show_float(x: float) -> str:
    return "%s" % x


def both(b: bool, x: float) -> str:
    return "[%s|%s]" % (b, x)


def padded_bool(b: bool) -> str:
    return "%10s" % b
