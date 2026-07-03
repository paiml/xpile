# PMAT-1092 (skeptic pass PMAT-1090, C-F1 — the WORST shape): `except ... as
# x` colliding with the enclosing LOOP variable. CPython deletes `x` at
# handler exit, so the post-handler read raises UnboundLocalError (exit 1);
# the emitted Rust let the for-binding show through — value + exit 0, a
# SILENT divergence (returned 3). The exact family PMAT-1085 closed for
# nested-loop bindings, now closed for `except as` bindings too. Refused at
# lowering via the same collision check as the pre-bound-local shape.
def loop_collide() -> int:
    total = 0
    for x in range(3):
        try:
            raise ValueError("boom")
        except ValueError as x:
            pass
        total = total + x
    return total
