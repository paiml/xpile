# PMAT-794 (HUNT-V18 EXC-003): math.sqrt of a negative (and math.log* of a
# non-positive) returned NaN/-inf silently — never panicked — so a guarding
# `except ValueError:` was dead code, where Python raises ValueError("math
# domain error"). Both backends now guard the domain and panic with the tagged
# ValueError, so the matching except catches it. Cross-checked vs python3.
import math


def guarded_sqrt() -> float:
    x = -1.0
    try:
        return math.sqrt(x)
    except ValueError:
        return -1.0


def guarded_log() -> float:
    x = 0.0
    try:
        return math.log(x)
    except ValueError:
        return -2.0


def ok_sqrt() -> float:
    return math.sqrt(16.0)


def wrong_handler() -> float:
    x = -4.0
    try:
        return math.sqrt(x)
    except KeyError:
        return 99.0
