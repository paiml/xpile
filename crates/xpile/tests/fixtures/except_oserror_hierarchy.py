# PMAT-1081 (skeptic-pass find): `except OSError:` must catch the tagged
# subclasses CPython's hierarchy implies — a missing-file read raises
# FileNotFoundError, which IS an OSError. Previously the OSError arm matched
# only the generic `xpile: OSError:` tag, so a FileNotFoundError silently
# re-raised past it (and `except IOError:`, the Python-3 alias, expanded to
# a leaf tag nothing emits — it could never catch anything at all).
def read_or_default(p: str) -> str:
    try:
        return open(p).read()
    except OSError:
        return "fallback"


def read_or_default_io(p: str) -> str:
    try:
        return open(p).read()
    except IOError:
        return "fallback"
