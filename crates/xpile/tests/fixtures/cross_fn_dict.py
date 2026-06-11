# PMAT-471 (R2): cross-function return-type inference. `s = make_scores()`
# must type `s` as the callee's declared dict[str,int] return, not the
# old hardcoded i64 fallback (which emitted `let s: i64` -> rustc error).
def make_scores() -> dict[str, int]:
    scores: dict[str, int] = {}
    scores["alice"] = 10
    scores["bob"] = 20
    return scores


def alice_score() -> int:
    s = make_scores()
    return s["alice"]


def total() -> int:
    s = make_scores()
    return s["alice"] + s["bob"]
