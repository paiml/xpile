# PMAT-1427: `xs[i].append(e)` / `d[k].append(e)` index disposition.
#
# The subscript-receiver append was the last member of the subscript family
# emitting the raw narrowing coercion `base[(i) as usize]`. `(-1i64) as usize`
# is `usize::MAX`, so every negative index panicked with Rust's NATIVE
# out-of-bounds message where CPython appends to the last sub-list, and an
# out-of-range index panicked UNTAGGED, which the typed-`except` filter could
# not route to `except IndexError`. The dict arm's `.unwrap()` was the same
# shape one container over: an untagged panic where CPython raises `KeyError`.
#
# Every row below is driven by the differential oracle against live CPython, so
# the wrap, the two tagged exceptions, and the unchanged non-negative fast path
# are all VALUE-checked rather than asserted. The boundary rows (`a[-3]` on a
# 3-list, `a[-1]` on a 1-list) are where a WRAP and a CLAMP-to-zero coincide —
# without them a corpus of only-diverging rows could not tell the two apart.


def main() -> None:
    # Negative index, literal and runtime — both wrap to `len + i`.
    a: list[list[int]] = [[1], [2], [3]]
    a[-1].append(99)
    i: int = -2
    a[i].append(88)
    print(a[2][1], a[1][1])

    # Boundary: the most-negative in-range index wraps to slot 0, where a
    # clamp-to-zero would agree — and one slot further is an IndexError.
    b: list[list[int]] = [[10], [20], [30]]
    b[-3].append(11)
    print(b[0][1])
    one: list[list[int]] = [[7]]
    one[-1].append(8)
    print(one[0][1])

    # Non-negative fast path, literal and runtime — unchanged by the fix.
    c: list[list[int]] = [[1], [2], [3]]
    c[0].append(4)
    j: int = 2
    c[j].append(5)
    print(c[0][1], c[2][1])

    # Out of range in BOTH directions raises a catchable IndexError.
    try:
        c[5].append(0)
    except IndexError:
        print("IndexError high")
    try:
        c[-4].append(0)
    except IndexError:
        print("IndexError low")
    print(len(c), len(c[0]))

    # Dict base: a present key appends, an absent key raises a catchable
    # KeyError carrying CPython's `repr(k)` payload.
    d: dict[str, list[int]] = {"a": [1]}
    d["a"].append(5)
    print(d["a"][1])
    try:
        d["zz"].append(9)
    except KeyError:
        print("KeyError")
    print(len(d))
