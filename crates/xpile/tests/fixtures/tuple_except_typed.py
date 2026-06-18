# PMAT-763 (HUNT-V16 #3): a tuple `except (A, B):` emitted a bare `Err(_) =>
# body` with NO type guard, so it swallowed ANY exception (incl. one not in the
# tuple), where Python only catches the listed types. The single-named `except
# E:` already built a re-raise denylist; the tuple form now does too — it
# re-raises a known exception NOT in the listed set. Cross-checked vs python3.


def wrong_tuple() -> int:
    # ZeroDivisionError is NOT in (KeyError, ValueError) → must propagate
    try:
        return 10 // 0
    except (KeyError, ValueError):
        return -1


def right_tuple() -> int:
    # int("abc") raises ValueError, which IS listed → caught
    try:
        return int("abc")
    except (KeyError, ValueError):
        return -7
