# PMAT-802 (HUNT-V19 CHAIN-2): `x in d` where x's type can never match the dict's
# key type (1 in dict[str,V], "k" in dict[int,V]) is always False in Python (no
# error), but emitted d.contains_key(&x) over a HashMap<K,_> with a different
# needle type → rustc E0308. It now folds to the constant (not in → True).
# int/bool stay tower-compatible (not folded). Cross-checked vs python3.


def int_in_strdict() -> bool:
    d: dict[str, int] = {"a": 1, "b": 2}
    return 1 in d


def str_in_intdict() -> bool:
    d: dict[int, str] = {1: "a", 2: "b"}
    return "x" in d


def str_notin_intdict() -> bool:
    d: dict[int, str] = {1: "a"}
    return "x" not in d


def real_hit() -> bool:
    d: dict[str, int] = {"a": 1}
    return "a" in d


def real_miss() -> bool:
    d: dict[str, int] = {"a": 1}
    return "z" in d
