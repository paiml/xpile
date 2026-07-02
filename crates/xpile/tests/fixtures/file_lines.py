# PMAT-1077 (file I/O, line iteration): `for line in open(P)` and
# `with open(P) as f: for line in f:` iterate the file's lines WITH their
# trailing "\n" (keepends) — new Expr::FileReadLines emits
# read_to_string(P)...split_inclusive('\n')..., which matches CPython
# text-mode file iteration exactly. Verified vs CPython (incl. keepends len).
def count_lines(p: str) -> int:
    n: int = 0
    for line in open(p):
        n += 1
    return n


def total_chars_with_newlines(p: str) -> int:
    total: int = 0
    for line in open(p):
        total += len(line)
    return total


def count_via_with(p: str) -> int:
    n: int = 0
    with open(p) as f:
        for line in f:
            n += 1
    return n
