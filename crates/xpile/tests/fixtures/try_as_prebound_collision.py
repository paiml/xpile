# PMAT-1092 (skeptic pass PMAT-1090, A-F5): `except ... as e` colliding with a
# PRE-EXISTING binding of `e`. CPython deletes the `as` name at handler exit,
# destroying the pre-existing binding only on the exception path (the later
# read raises UnboundLocalError only when the handler RAN) — path-dependent
# semantics the value model can't express. The emitted Rust block-scoped the
# `as` binding and let the OLD value survive: a SILENT divergence (returned
# "old" where CPython exits 1). Now refused at lowering.
def as_survives() -> str:
    e = "old"
    try:
        raise ValueError("boom")
    except ValueError as e:
        pass
    return e
