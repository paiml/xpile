# PMAT-625: a 1-element tuple type/value emitted Rust `(T)` / `(x)` — just a
# parenthesized value, NOT a real 1-tuple `(T,)` / `(x,)` — so `.0`/indexing
# failed to compile (E0610). Both the type emitter and the tuple-literal emitter
# now add the trailing comma for a single element. Multi-element tuples are
# unaffected. Also un-blocks 1-tuple f-string repr (PMAT-624).
def idx() -> int:
    p: tuple[int] = (42,)
    return p[0]


def from_param(p: tuple[int]) -> int:
    return p[0]


def repr1(p: tuple[int]) -> str:
    return f"{p}"


def pair(p: tuple[int, int]) -> int:
    return p[0] + p[1]
