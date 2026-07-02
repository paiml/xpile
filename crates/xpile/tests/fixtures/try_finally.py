# PMAT-1070: `finally:` on a statement-form try/except — runs in EVERY exit
# path (clean body, matched handler, handler-raised, unmatched-propagate).
# Emitted by wrapping the whole try/except in an OUTER catch_unwind: run F
# after, then re-propagate if the inner still panicked (so F runs even when a
# handler itself raises, and BEFORE the exception reaches an enclosing try).
# Works with single + multiple except. Deferred: finally-ONLY (no except —
# needs a no-catch encoding distinct from `except: pass`), and `else`.
# Differentially verified vs CPython (tf/tef/kF/1).
def clean_path() -> str:
    log: str = ""
    try:
        log = log + "t"
    except ValueError:
        log = log + "e"
    finally:
        log = log + "f"
    return log


def caught_path() -> str:
    log: str = ""
    try:
        log = log + "t"
        raise ValueError("v")
    except ValueError:
        log = log + "e"
    finally:
        log = log + "f"
    return log


def multi_except_finally() -> str:
    log: str = ""
    try:
        raise KeyError("k")
    except ValueError:
        log = "v"
    except KeyError:
        log = "k"
    finally:
        log = log + "F"
    return log


def finally_runs_before_propagate() -> int:
    trace: list[int] = []
    result: int = -1
    try:
        try:
            raise IndexError("i")
        except ValueError:
            trace.append(1)
        finally:
            trace.append(2)
    except IndexError:
        result = len(trace)
    return result

