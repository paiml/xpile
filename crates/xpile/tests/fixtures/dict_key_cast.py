# PMAT-852 (HUNT-V28 #4): a dict key that lowers to a bare cast — len(w) →
# w.chars().count() as i64 — emitted insert(... as i64.clone(), ...), which rustc
# parses as `as (i64.clone())` ("cast cannot be followed by a method call"). The
# key is now parenthesized before .clone(). Covers the dict-comp and d[k]=v forms.
# Cross-checked vs python3.


def comp_len_key() -> int:
    words = ["hi", "yo", "bye"]
    d = {len(w): w for w in words}
    return len(d)


def subscript_len_key() -> int:
    words = ["a", "bb", "ccc"]
    out: dict[int, int] = {}
    for w in words:
        out[len(w)] = 0
    return len(out)


def abs_key() -> int:
    d: dict[int, str] = {}
    d[abs(-5)] = "x"
    return len(d)
