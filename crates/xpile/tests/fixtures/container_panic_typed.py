# PMAT-747 (HUNT-V14 #2 exc-untagged-panic-swallowed): a dict-index miss
# (KeyError), an empty list.pop() / absent dict.pop(k) (IndexError / KeyError)
# emitted UNTAGGED native panics (HashMap `Index`, `Option::unwrap`), so the
# typed-`except` re-raise filter (PMAT-731, which only re-raises panics tagged
# `xpile: <KnownExc>:`) let an unrelated `except` SILENTLY SWALLOW them, where
# Python propagates. Each container-access panic is now tagged, so the matching
# `except` catches it and every other typed `except` re-raises it. Cross-checked
# vs python3 (continues the PMAT-743/744 typed-exception line).


def dict_miss_right(d: dict[str, int]) -> int:
    # except KeyError catches a dict-index miss
    try:
        return d["b"]
    except KeyError:
        return -7


def empty_pop_right(xs: list[int]) -> int:
    # except IndexError catches an empty list.pop()
    try:
        return xs.pop()
    except IndexError:
        return -3


def dict_pop_miss_right(d: dict[str, int]) -> int:
    # except KeyError catches an absent d.pop(k)
    try:
        return d.pop("z")
    except KeyError:
        return -9


def dict_miss_wrong(d: dict[str, int]) -> int:
    # except ValueError must NOT catch a KeyError — Python propagates it
    try:
        return d["b"]
    except ValueError:
        return -1


def dict_hit(d: dict[str, int]) -> int:
    # an in-bounds dict access is unchanged
    return d["a"]
