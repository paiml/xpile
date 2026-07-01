# PMAT-1011 (sweep #7): CPython requires the rjust/ljust/center fill to be
# EXACTLY one character — "TypeError: the fill character must be exactly one
# character long" — validated at the call, even when the string is already
# wide enough. The old emit silently `.repeat`ed a multi-char fill
# ("ab".center(5, "xy") gave "xyabxy"-style output where CPython raises):
# a SILENT-acceptance divergence.
def ok(s: str) -> str:
    return s.rjust(5, "0")


def bad_center(s: str) -> str:
    return s.center(8, "xy")


def bad_nopad(s: str) -> str:
    return s.ljust(2, "xy")
