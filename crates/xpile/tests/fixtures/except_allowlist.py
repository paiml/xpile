# PMAT-789 (HUNT-V18 EXC-001): the typed-`except` discriminator was a BLOCKLIST
# (re-raise only the 6 OTHER cataloged builtins, catch everything else), so an
# `except ValueError:` silently SWALLOWED a RuntimeError / any non-cataloged or
# untagged exception where Python propagates. It is now an ALLOWLIST: catch iff
# the panic tag names one of the handler's OWN listed types, else re-raise.
# Cross-checked vs python3.


def trigger_runtime() -> int:
    # mutating a dict's size mid-iteration → RuntimeError (PMAT-743).
    d = {1: 1, 2: 2, 3: 3}
    for k in d:
        d[k + 100] = 0
    return len(d)


def swallow_attempt() -> int:
    # except ValueError must NOT catch the RuntimeError — it propagates.
    try:
        return trigger_runtime()
    except ValueError:
        return -1


def runtime_caught() -> int:
    # except RuntimeError NOW catches it (was uncatchable under the blocklist).
    try:
        return trigger_runtime()
    except RuntimeError:
        return 77


def zde_caught() -> int:
    # a matching handler still catches its own type (regression guard).
    try:
        return 10 // 0
    except ZeroDivisionError:
        return -99


def bare_catch() -> int:
    # a bare except: still catches everything.
    try:
        return 10 // 0
    except:
        return -5
