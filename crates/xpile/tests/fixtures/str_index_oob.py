# PMAT-801 (HUNT-V19 STR-IDX-OOB): a string index out of range (s[i], i beyond
# the end) panicked with Rust's raw "index out of bounds" message instead of the
# tagged xpile: IndexError:, so under the allowlist except (PMAT-789) a typed
# `except IndexError` couldn't catch it — it wrongly propagated where Python
# returns the handler value. The string-index read now bounds-checks with the
# tagged panic (mirror of the list-index tagging). Cross-checked vs python3.


def catch_oob(s: str, i: int) -> str:
    try:
        return s[i]
    except IndexError:
        return "OOB"


def normal(s: str, i: int) -> str:
    return s[i]


def neg_ok(s: str) -> str:
    return s[-1]
