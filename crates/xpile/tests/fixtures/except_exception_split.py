# PMAT-1105 (b)+(c) (skeptic pass PMAT-1098, C-F2/C-F3): the Exception-vs-
# BaseException split and refusal honesty. (b) SystemExit derives
# BaseException only, so `except Exception:` must let it propagate (CPython:
# stderr `bye`, exit 1) while a bare `except:` catches it. (c) WORST-CLASS:
# xpile's own capability/honesty panics (e.g. the C-PY-INT-ARITH i64-overflow
# refusal) are NOT Python exceptions — no exception exists on the Python side
# at all (CPython computes 92233720368547758000) — so EVERY handler,
# including bare `except:` and `except Exception:`, must re-raise them; the
# old unconditional catch-all arm silently DEFEATED the loud-refusal
# guarantee. Cross-checked vs python3.
def exc_no_systemexit() -> int:
    r = 0
    try:
        raise SystemExit("bye")
    except Exception:
        r = 1
    return r


def bare_catches_systemexit() -> int:
    r = 0
    try:
        raise SystemExit("bye")
    except:
        r = 2
    return r


def exc_catches_valueerror() -> int:
    r = 0
    try:
        raise ValueError("x")
    except Exception:
        r = 3
    return r


def refusal_not_swallowed(a: int) -> int:
    r = 0
    try:
        r = a * 1000
    except Exception:
        r = -1
    return r


def bare_refusal(a: int) -> int:
    r = 0
    try:
        r = a * 1000
    except:
        r = -1
    return r
