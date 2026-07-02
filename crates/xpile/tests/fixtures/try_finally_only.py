# PMAT-1073: `try: B finally: F` with NO `except` — finally-only. Runs cleanup
# in every path and PROPAGATES any exception (does NOT swallow, unlike
# `except: pass`). Emitted via the `finally_only` flag: no handler dispatch —
# `{ let r = catch_unwind(|| B); F; if let Err(e) = r { resume_unwind(e) } }`.
# Unblocks context managers (PMAT-1072). Verified vs CPython.
def cleanup_runs() -> str:
    log: str = "body"
    try:
        log = log + "-t"
    finally:
        log = log + "-clean"
    return log


def multi_cleanup() -> int:
    xs: list[int] = []
    try:
        xs.append(1)
        xs.append(2)
    finally:
        xs.append(99)
    return len(xs) * 100 + xs[2]
