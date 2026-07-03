# PMAT-1082 (skeptic-pass find, PMAT-1081 probe p05): `continue` in an
# `except` HANDLER inside `for i in range(n)` hung FOREVER — the range-for
# desugars to a while with its counter-increment at body-END, and the
# continue-increment rewrite (PMAT-799) did not recurse into TryCatch
# handler blocks, so the rewritten loop re-entered without incrementing.
# The rewrite now descends handler / extra-handler / finally blocks (all
# emitted at loop scope; the try BODY is a catch_unwind closure and refuses
# top-level continue instead). Verified vs CPython (8 / 331).
def skip_bad(n: int) -> int:
    total: int = 0
    for i in range(n):
        try:
            if i == 2:
                raise ValueError("bad")
            total = total + i
        except ValueError:
            continue
    return total


# `continue` in a FINALLY block (legal Python since 3.8) — the finally is
# emitted at function scope after the outer catch_unwind, so the injected
# increment + continue re-enters the loop correctly (and skipping the
# trailing resume_unwind matches CPython's swallow-on-continue semantics).
def finally_continue(n: int) -> int:
    total: int = 0
    for i in range(n):
        try:
            if i == 2:
                raise ValueError("bad")
            total = total + 10
        except ValueError:
            total = total + 1
        finally:
            if i == 0:
                continue
        total = total + 100
    return total
