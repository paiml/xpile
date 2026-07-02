# PMAT-1075 (file I/O, 2nd increment): `open(path, "w").write(content)` as a
# statement writes a whole str to a file (truncating), emitted inline as
# `std::fs::write(&(path), &(content))` (borrowed via AsRef so a variable
# path/content isn't moved) with a panic on error. Append mode "a" refuses
# precisely. Round-trips with `open(path).read()` (PMAT-1074). Verified vs
# CPython (write-then-read).
def write_then_read(p: str, s: str) -> int:
    open(p, "w").write(s)
    return len(open(p).read())


def overwrite(p: str) -> str:
    open(p, "w").write("first-longer")
    open(p, "w").write("second")
    return open(p).read()
