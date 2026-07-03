# PMAT-1092 (skeptic pass PMAT-1090): the SUPPORTED try-scoping patterns the
# new refusals must NOT catch. `flagged` is the workaround the refusal message
# recommends — bind before the `try`, reassign inside the arms; the closure
# mutates the outer `let mut`, and a raise MID-body leaves the partial
# assignment in place exactly like CPython (x=1 already applied when the raise
# fires, then the handler overwrites to -1). `seq_as_reuse` re-uses one `as e`
# name across TWO sequential trys with reads only INSIDE each handler — legal
# in both models (each handler scopes its own binding; no live collision at
# either `except` since the first try's `as` binding is gone by the second) —
# proving the PMAT-1092 collision refusal doesn't false-positive on it.
# CPython ground truth: flagged(1)=2 flagged(-1)=-1;
# seq_as_reuse(-1,-1)="firstsecond" (1,-1)="second" (1,1)="".
def flagged(n: int) -> int:
    x = 0
    try:
        x = 1
        if n < 0:
            raise ValueError("neg")
        x = 2
    except ValueError:
        x = -1
    return x


def seq_as_reuse(a: int, b: int) -> str:
    out = ""
    try:
        if a < 0:
            raise ValueError("first")
    except ValueError as e:
        out = out + str(e)
    try:
        if b < 0:
            raise ValueError("second")
    except ValueError as e:
        out = out + str(e)
    return out
