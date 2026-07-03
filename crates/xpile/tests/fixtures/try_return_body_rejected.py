# PMAT-1082 (skeptic-pass find, PMAT-1081 probe p26): `return` inside a
# statement-form try BODY returned from the catch_unwind CLOSURE, not the
# function — this printed 0 where CPython prints 5, a SILENT miscompile.
# Now refused at lowering with a precise message (the value form
# `try: return X except: return Y` stays supported).
def early() -> int:
    try:
        x: int = 1
        return 5
    except ValueError:
        pass
    return 0
