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
