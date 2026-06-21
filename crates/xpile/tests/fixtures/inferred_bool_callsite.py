# PMAT-856 (HUNT-V28 #3): a function with an inferred (unannotated) bool return —
# def gt0(x): return x > 0 — got `-> bool` in its emitted signature, but the
# call-site FnSig defaulted to i64, so `r = gt0(5)` bound i64 (E0308), `if gt0(5)`
# added `!= 0i64` (E0308), and str(gt0(5)) printed "true" not "True" (silent-wrong).
# The pre-pass now infers bool for a trailing comparison / not / bool-literal
# return. Cross-checked vs python3.


def gt0(x: int):
    return x > 0


def not_pred(x: int):
    return not (x > 0)


def use_let() -> int:
    r = gt0(5)
    return 1 if r else 0


def use_cond() -> int:
    if gt0(5):
        return 7
    return 0


def use_str() -> str:
    return str(gt0(5)) + "," + str(not_pred(5))
