# PMAT-1078 (file I/O, append): `open(P, "a").write(S)` and
# `with open(P, "a") as f: f.write(S)` append (create-if-absent) instead of
# truncating — Stmt::FileWrite gains an `append` flag emitting
# OpenOptions::new().create(true).append(true) + write_all. Verified vs
# CPython (append accumulates; "w" still truncates).
def build_log(p: str) -> str:
    open(p, "w").write("")
    open(p, "a").write("A")
    open(p, "a").write("B")
    with open(p, "a") as f:
        f.write("C")
    return open(p).read()
