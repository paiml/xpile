# PMAT-699: a bare-variable key or value in a dict literal is moved into
# `m.insert(...)`; reusing it afterward was rustc E0382. The DictLit codegen now
# clones bare idents at insert. Literals/temporaries are emitted as-is.
def kv(k: str, v: int) -> int:
    d = {k: v}
    return d[k]


def with_str_val(s: str) -> int:
    d = {"key": s}
    return len(d["key"])


def two_pairs(a: str, b: str) -> int:
    d = {a: 1, b: 2}
    return len(d)


def int_keys(p: int, q: int) -> int:
    d = {p: 1, q: 2}
    return len(d)
