# PMAT-944 (correctness-hunt): a MIXED / non-bool `and`/`or` in VALUE position
# returns Python's union operand value (`0 or "d"` is `"d"`, an int|str union),
# which has no single Rust type. xpile previously folded it to a Rust `bool`,
# silently diverging (`return 0 or "d"` produced `true`/`false`, never the
# operand). It is now REJECTED with a clear diagnostic. (Same-typed operands and
# the boolean-context forms still work — see oracle fixture `boolop_value_union`.)


def union_or(n: int) -> str:
    return n or "default"
