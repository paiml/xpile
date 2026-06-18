# PMAT-805 (HUNT-V20): a comprehension's tuple loop vars (k, v in d.items()) were
# registered at FUNCTION scope, so a later same-named real for-loop emitted a
# bare `k = __fe0` assignment to a never-`let`-declared name → rustc E0425. Comp
# loop vars are now marked loop-scoped (comp-block-scoped, not durable), so the
# later for-loop binds a fresh `for k`. Cross-checked vs python3.


def dict_comp_then_loop() -> int:
    src = {"a": 1, "b": 2}
    doubled = {k: v * 2 for k, v in src.items()}
    total = 0
    for k in ["a", "b"]:
        total = total + doubled[k]
    return total


def list_comp_then_loop() -> int:
    src = {"a": 10, "b": 20}
    pairs = [k for k, v in src.items()]
    total = 0
    for k in ["a", "b"]:
        total += len(k)
    return total + len(pairs)
