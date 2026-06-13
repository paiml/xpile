def money(x: float) -> str:
    return "$%.2f" % x


def rjust_num(n: int) -> str:
    return "[%5d]" % n


def ljust_num(n: int) -> str:
    return "[%-5d]" % n


def zero_pad(n: int) -> str:
    return "%05d" % n


def rjust_str(s: str) -> str:
    return "[%5s]" % s


def width_prec(x: float) -> str:
    return "%8.2f" % x


def signed(n: int) -> str:
    return "%+d" % n
