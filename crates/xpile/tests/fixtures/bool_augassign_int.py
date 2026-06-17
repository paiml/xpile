# PMAT-746 (HUNT-V14 bool-augadd-i64-coerce): Python's `bool` is an `int`
# subtype, so an augmented assignment with a bool operand into an int
# accumulator is ordinary integer arithmetic — `count += a == b` counts matches.
# xpile's plain `total + (a == b)` path already coerced the bool to i64, but the
# AUGMENTED path (`+=`/`-=`/`*=`) emitted `(count).checked_add(<bool>)` with no
# cast → rustc E0308. The fix mirrors the plain arm's `to_i64_operand` coercion.
# `&`/`|`/`^` over two bools must stay a bool op (PMAT-580). Cross-checked vs
# python3.


def count_spaces(s: str) -> int:
    # the canonical "count how many match a predicate" idiom
    spaces: int = 0
    for ch in s:
        spaces += ch == " "
    return spaces


def acc_mul(n: int, flag: bool) -> int:
    total: int = n
    total *= flag
    return total


def sub_bool(n: int, b: bool) -> int:
    total: int = n
    total -= b
    return total


def subscript_bool(b: bool) -> int:
    # a subscript target augmented-assigned with a bool RHS
    xs: list[int] = [10]
    xs[0] += b
    return xs[0]


def bool_and(a: bool, c: bool) -> bool:
    # bitwise &= over two bools stays a bool (NOT coerced to i64)
    flag: bool = a
    flag &= c
    return flag
