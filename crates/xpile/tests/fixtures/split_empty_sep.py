# PMAT-922 (HUNT-V16 #29): `s.split("")` / `s.split("", n)` / `s.rsplit("", n)`
# with an EMPTY separator now raises ValueError at the call (like Python's
# `ValueError: empty separator`) instead of silently returning a bogus list (the
# old `str::split("")` yielded `["", "a", "b", "c", ""]` for `"abc"` — a 5-element
# list with no error). Valid separators are unchanged. See
# `split_empty_sep_emitted_rust_matches_cpython` in transpile_e2e.rs.
def split_ok(s: str) -> int:
    parts: list[str] = s.split("-")
    return len(parts)


def splitn_ok(s: str) -> int:
    parts: list[str] = s.split("-", 1)
    return len(parts)


def rsplit_ok(s: str) -> str:
    parts: list[str] = s.rsplit("-", 1)
    return parts[0]


def split_empty(s: str, sep: str) -> int:
    parts: list[str] = s.split(sep)
    return len(parts)
