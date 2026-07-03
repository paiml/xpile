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


# Loop accumulators with COMPUTED values — the dominant real-world idiom. The
# element type comes from typing the appended EXPRESSION (via the same oracle the
# lowering uses), with the loop variable's type (`i: int` from range) in scope.
def build_str_loop() -> list[str]:
    xs = []
    for i in range(3):
        xs.append(str(i))  # str(i) → Str: the str-in-loop trap (must keep quotes)
    return xs


def build_computed_int_loop() -> list[int]:
    xs = []
    for i in range(3):
        xs.append(i * i)  # i*i → I64
    return xs


def build_computed_dict_loop() -> dict[int, int]:
    d = {}
    for i in range(3):
        d[i] = i * i  # key i → I64, value i*i → I64
    return d


def build_from_param(n: int) -> list[int]:
    xs = []
    xs.append(n)  # appended param → the param's type
    return xs
