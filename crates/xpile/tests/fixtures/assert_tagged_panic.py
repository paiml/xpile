# PMAT-788 (HUNT-V17 #4): a failed `assert` raises Python AssertionError. A bare
# `assert!(cond, "{}", msg)` panicked with an UNTAGGED message, so the typed
# `except` re-raise filter (which only re-raises `xpile: <KnownExc>:` payloads)
# let an unrelated `except ValueError:` SWALLOW it (silent-wrong; Python
# propagates). The assert now emits a tagged `xpile: AssertionError:` panic, and
# AssertionError is in KNOWN_EXC, so `except AssertionError` catches it and any
# other typed except re-raises it. Cross-checked vs python3.
def must_pos(n: int) -> int:
    assert n >= 0, "neg"
    return n

def caught_by_assert(n: int) -> int:
    try:
        return must_pos(n)
    except AssertionError:
        return -2

def wrong_except(n: int) -> int:
    try:
        return must_pos(n)
    except ValueError:
        return -1
