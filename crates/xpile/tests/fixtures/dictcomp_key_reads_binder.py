# PMAT-826 (HUNT-V25 #1): a dict comprehension {KEY(w): w for w in words} where
# the value is the bare loop binder and the key re-reads it (w[0]) — the DictSet
# codegen binds the value first, MOVING the non-Copy String w, before the key
# re-reads w → rustc E0382. The bare-binder value is now cloned. A transformed
# value (w.upper()) or a Copy binder is unaffected. Cross-checked vs python3.


def first_char_keys() -> int:
    words = ["apple", "banana", "cherry", "avocado"]
    d = {w[0]: w for w in words}
    return len(d)  # keys a,b,c → 3 (avocado overwrites apple)


def transformed_value() -> int:
    words = ["aa", "bb"]
    d = {w[0]: w.upper() for w in words}
    return len(d)


def int_binder() -> int:
    d = {x: x * x for x in range(4)}
    return d[3]
