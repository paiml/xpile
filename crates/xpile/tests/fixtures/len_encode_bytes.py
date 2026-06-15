# PMAT-674: `len(s.encode())` is the UTF-8 BYTE length of a str (NOT the
# code-point count `len(s)` gives) — equals Rust `String::len()`.
def byte_len(s: str) -> int:
    return len(s.encode())


def byte_len_utf8(s: str) -> int:
    return len(s.encode("utf-8"))


def char_len(s: str) -> int:
    return len(s)
