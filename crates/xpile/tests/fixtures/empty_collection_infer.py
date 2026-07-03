# PMAT-1160: empty-collection first-use element inference.
#
# Each local is initialised with an EMPTY collection (`[]` / `{}`) and its
# element / key-value type is inferred from the first LITERAL element-revealing
# use (`.append(...)`, `d[k] = v`). The inference happens in the FRONTEND type
# model, so the emitted Rust is typed `Vec<String>` / `IndexMap<i64, String>` /
# ... — NOT a `List[int]` default that a codegen-only band-aid would leave (it
# would still compile but strip repr quotes, silently diverging from CPython).
# The str-list case is the "trap": it must type as `Vec<String>`.
def build_int_list() -> list[int]:
    xs = []
    xs.append(3)
    xs.append(4)
    return xs


def build_str_list() -> list[str]:
    xs = []
    xs.append("a")
    xs.append("b")
    return xs


def build_int_dict() -> dict[int, str]:
    d = {}
    d[1] = "a"
    d[2] = "b"
    return d


def build_str_dict() -> dict[str, int]:
    d = {}
    d["x"] = 1
    d["y"] = 2
    return d
