# PMAT-456 / v0.2.0 Track 1.B: list[bool] literal. First fixture to
# exercise Python `True` / `False` literals through the new
# Expr::LitBool meta-HIR variant. Backends emit lowercase `true` /
# `false` (Rust/Ruchy) and capitalised `True` / `False` (Lean).
def flags() -> list[bool]:
    return [True, False, True]
