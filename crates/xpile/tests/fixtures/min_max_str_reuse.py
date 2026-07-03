# PMAT-1167: min()/max() of two str operands lowers to `.min()`/`.max()`, which
# CONSUME their operands (String: Ord, by value). A bare `min(a, b)` moved a/b,
# so a later read (`print(a)`) was rustc E0382 (accept-then-fail). Fix: route the
# min/max args through the canonical clone-if-reused helper — a no-op for Copy
# operands and single-use bindings, cloning only reused non-Copy operands.
def pick(a: str, b: str) -> str:
    m = min(a, b)
    # a and b are READ again after min() consumed them
    return m + a + b
