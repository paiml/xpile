# PMAT-1059: MULTIPLE `except` clauses on a statement-form try —
# `try: B except E1: H1 except E2: H2 except: H3`. The Err arm of the
# catch_unwind emits an ordered if/else-if chain over the handlers (the first
# matching type wins, source order); a catch-all (`except:` / `except
# Exception:`, empty types — Python-required last) is the final `else`, else
# the chain ends in resume_unwind (an unmatched exception PROPAGATES, not
# swallowed). Extends Stmt::TryCatch additively (extra_handlers, serde-default
# so single-except IR is unchanged). Verified vs CPython (val/key/other/key/idx).
def raise_kind(kind: int) -> int:
    if kind == 1:
        raise ValueError("v")
    if kind == 2:
        raise KeyError("k")
    return 1 // 0


def classify(kind: int) -> str:
    result: str = "none"
    try:
        raise_kind(kind)
    except KeyError:
        result = "key"
    except ValueError:
        result = "val"
    except:
        result = "other"
    return result


def ordered_first_wins() -> str:
    d: dict[str, int] = {}
    result: str = "?"
    try:
        _ = d["missing"]
    except KeyError:
        result = "key"
    except Exception:
        result = "exc"
    return result


def tuple_then_single() -> str:
    result: str = "?"
    try:
        raise IndexError("i")
    except (KeyError, ValueError):
        result = "kv"
    except IndexError:
        result = "idx"
    return result

