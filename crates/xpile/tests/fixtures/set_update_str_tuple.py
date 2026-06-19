# PMAT-845 (HUNT-V27 #5): set.update(<str>) / set.update(<tuple>) emitted
# .iter() on a String / tuple → rustc E0599 (a str/tuple is iterable in Python
# but not a Vec in Rust). A str arg now materializes to its chars-as-strings,
# a homogeneous tuple to a list of its elements; a set/list arg is unchanged.
# Cross-checked vs python3.


def from_str() -> int:
    s = {"x"}
    s.update("abc")
    return len(s)


def from_tuple() -> int:
    s = {1, 2}
    s.update((3, 4, 4))
    return len(s)


def from_set(other: set[int]) -> int:
    s = {1, 2}
    s.update(other)
    return len(s)


def from_list() -> int:
    s = {1, 2}
    s.update([5, 6, 6])
    return len(s)
