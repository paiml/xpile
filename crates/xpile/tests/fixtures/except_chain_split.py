# PMAT-1105 (b)+(c) on the MULTI-except chain: `except Exception` is an
# ordinary discriminated arm (the frontend sentinel tag), so a later bare
# `except:` stays REACHABLE and catches the SystemExit that Exception must
# decline (CPython: chain(2) == "other"); the bare arm itself is gated, so a
# capability refusal re-raises past the WHOLE chain. Cross-checked vs python3.
def chain(kind: int) -> str:
    result = "none"
    try:
        if kind == 1:
            raise ValueError("v")
        if kind == 2:
            raise SystemExit("bye")
        if kind == 3:
            result = "big"
    except KeyError:
        result = "key"
    except Exception:
        result = "exc"
    except:
        result = "other"
    return result


def chain_refusal(a: int) -> int:
    r = 0
    try:
        r = a * 1000
    except KeyError:
        r = -1
    except:
        r = -2
    return r


def expr_bare(a: int, b: int) -> int:
    # expression-form try: the gated catch-all still catches real Python
    # exceptions (ZeroDivisionError) …
    try:
        return a // b
    except:
        return -1


def expr_bare_refusal(a: int) -> int:
    # … and re-raises capability refusals.
    try:
        return a * 1000
    except:
        return -1
