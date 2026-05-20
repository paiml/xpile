# PMAT-450 / v0.2.0 Track 1.A: str parameter passthrough — exercises
# Type::Str at parameter position (the foundation PR enabled return
# position; this fixture proves the param path works too).
# Governing contract: C-XLATE-PY-STR-TO-RUST-STRING.
def echo(name: str) -> str:
    return name
