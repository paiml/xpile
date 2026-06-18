# PMAT-774 (HUNT-V16 CG-5): max()/min() over an empty sequence (e.g. an empty
# filtered comprehension) raises Python `ValueError: max() arg is an empty
# sequence`. The int/Ord branch emitted a bare `.unwrap()` (native panic), and
# the float branch's message lacked the `xpile: ValueError:` prefix — so neither
# was caught by a typed `except ValueError`. Both branches now emit the
# canonical tagged message. Cross-checked vs python3.


def max_empty_caught() -> int:
    xs = [1, 2, 3]
    try:
        return max(x for x in xs if x > 100)
    except ValueError:
        return -1


def max_empty_wrong_except() -> int:
    xs = [1, 2, 3]
    try:
        return max(x for x in xs if x > 100)
    except KeyError:
        return -2


def min_nonempty() -> int:
    return min([3, 1, 2])
