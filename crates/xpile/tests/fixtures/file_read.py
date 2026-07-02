# PMAT-1074 (first increment): file I/O — `open(path).read()` reads a whole
# file to a str, emitted inline as `std::fs::read_to_string(path)` with a
# panic on error (missing file → xpile: FileNotFoundError, matching CPython so
# both raise). Chained str methods work on the result. Follow-ups (roadmap
# PMAT-1074): write, `with open() as f`, line iteration. Verified vs CPython.
def read_all(p: str) -> str:
    return open(p).read()


def line_count(p: str) -> int:
    return len(open(p).read().splitlines())


def char_count(p: str) -> int:
    return len(open(p).read())


# PMAT-1081 (skeptic-pass find): the path VARIABLE stays usable after the
# read — the read/modify/write idiom. The emit borrows `&(p)`; taking `p`
# by value moved it into read_to_string (E0382 on this exact shape).
def read_then_reuse(p: str) -> int:
    s: str = open(p).read()
    return len(s) + len(p)
