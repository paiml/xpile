# PMAT-1076 (file I/O, idiomatic form): `with open(P[, mode]) as f: BODY`.
# The single f.read()/f.write(s) is substituted with open(P)/open(P,"w") and
# the `with` is unwrapped (read_to_string / fs::write each open+op+close =
# a single-op handle). Multiple ops / `for line in f` / append refuse (would
# diverge). Round-trips vs CPython.
def read_len(p: str) -> int:
    with open(p) as f:
        content = f.read()
    return len(content)


def read_lines(p: str) -> int:
    with open(p) as f:
        lines = f.read().splitlines()
    return len(lines)


def write_then_read(p: str, s: str) -> str:
    with open(p, "w") as f:
        f.write(s)
    with open(p) as g:
        return g.read()
