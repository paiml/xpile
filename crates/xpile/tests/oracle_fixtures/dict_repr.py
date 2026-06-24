# PMAT-920 (HUNT-V17 #16 / CF-2): `str(d)` over a dict-typed value must render
# Python's `{k: v, ...}` repr — string keys/values quoted, ints bare, the empty
# dict as `{}` — and match CPython byte-for-byte. The backing IndexMap iterates in
# insertion order, the same order CPython 3.7+ preserves. Before PMAT-920 this
# mis-inferred I64 and the `-> str` return rejected ("body produces I64").
def str_dict() -> str:
    d = {"a": 1, "b": 2, "c": 3}
    return str(d)


def int_keyed() -> str:
    d = {1: "x", 2: "y"}
    return str(d)


def empty_dict() -> str:
    d: dict[str, int] = {}
    return str(d)


def single_entry() -> str:
    d = {"only": 42}
    return str(d)


def main() -> None:
    print(str_dict())
    print(int_keyed())
    print(empty_dict())
    print(single_entry())
