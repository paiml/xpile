# PMAT-1091 (skeptic pass PMAT-1090, A-F2/A-F3): KeyError payload repr
# remainder. (a) COMPOSITE keys: a tuple-key miss fell back to Rust `{:?}`
# Debug, which double-quotes contained strings — `d[("b", 2)]` miss printed
# `("b", 2)` where CPython's `str(KeyError(k))` is `repr(k)` = `('b', 2)` —
# and lowercases bools. (b) NON-PRINTABLES: the repr escape predicate only
# covered C0/0x7F/C1, so a key containing U+2028 emitted the raw line
# separator where CPython escapes every non-printable (` `), with
# width-matched `\x`/`\u`/`\U` forms. Every expected payload in the driver
# is CPython 3.10 ground truth (verified via python3).


def miss_tuple_si() -> str:
    d = {("a", 1): 2}
    try:
        return str(d[("b", 2)])
    except KeyError as e:
        return str(e)


def miss_tuple_ss() -> str:
    d = {("a", "b"): 1}
    try:
        return str(d[("x", "y")])
    except KeyError as e:
        return str(e)


def miss_nested_tuple() -> str:
    d = {(("a", 1), 2): 1}
    try:
        return str(d[(("b", 3), 4)])
    except KeyError as e:
        return str(e)


def miss_bool_tuple() -> str:
    d = {(True, "q"): 1}
    try:
        return str(d[(False, "z")])
    except KeyError as e:
        return str(e)


def miss_three_tuple() -> str:
    d = {("a", 1, True): 2}
    try:
        return str(d[("b", 2, False)])
    except KeyError as e:
        return str(e)


def pop_miss_tuple() -> str:
    d = {("a", 1): 2}
    try:
        return str(d.pop(("c", 9)))
    except KeyError as e:
        return str(e)


def set_remove_miss_tuple() -> str:
    s = {("a", 1)}
    try:
        s.remove(("b", 2))
    except KeyError as e:
        return str(e)
    return "removed"


def miss_u2028_key() -> str:
    d = {"k": 1}
    try:
        return str(d["x y"])
    except KeyError as e:
        return str(e)


def miss_nbsp_quote_key() -> str:
    # `'` in the key quote-switches the repr to double quotes; the U+00A0
    # (Zs, < 0x100) must escape as `\xa0` inside it.
    d = {"k": 1}
    try:
        return str(d["it's\xa0ok"])
    except KeyError as e:
        return str(e)


def repr_separators() -> str:
    # repr() rides the same block: Zl U+2028, Cf U+00AD (soft hyphen),
    # Cf U+200B (zero-width space) all escape width-matched.
    return repr("a b\xadc​d")


def repr_astral_private() -> str:
    # Co private-use plane 15 escapes in CPython's 8-hex `\U` form.
    return repr("p\U000f0001q")
