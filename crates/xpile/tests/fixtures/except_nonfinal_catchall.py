# PMAT-1082 (skeptic-pass find, PMAT-1081 probe p09): a catch-all
# `except Exception:` in NON-final position (legal Python — only bare
# `except:` is syntax-required last) emitted a bare block followed by a
# dangling `else if` (invalid Rust). It now terminates the if/else-if
# chain and DROPS the later arms — they are unreachable in CPython too,
# since Exception catches everything xpile models. Verified vs CPython
# (any / any / clean).
def classify(k: int) -> str:
    result: str = "clean"
    try:
        if k == 0:
            raise ValueError("v")
        if k == 1:
            raise KeyError("k")
    except Exception:
        result = "any"
    except ValueError:
        result = "val"
    return result
