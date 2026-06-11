# PMAT-466 regression (review #2/#4/#7): dict reads `d[k]` in expression
# positions NOT directly recursed by lower_expr_in_ctx — a call
# argument, a relational comparison, a ternary branch, and a len()
# argument. The post-lowering rewrite_dict_reads pass must turn each
# `Expr::Index` over a dict into `Expr::DictGet` (emitting
# `d[&(k)].clone()`), never a list index (`d[k as usize]`, which a
# HashMap cannot be indexed by).
def identity(x: int) -> int:
    return x


def via_call(table: dict[int, int], k: int) -> int:
    return identity(table[k])


def is_positive(table: dict[int, int], k: int) -> bool:
    return table[k] > 0


def pick(table: dict[int, int], k: int, n: int) -> int:
    return table[k] if n > 0 else 0


def val_len(table: dict[int, str], k: int) -> int:
    return len(table[k])


def lookup_or(table: dict[int, int], k: int) -> int:
    # The lookup-with-fallback idiom: a dict read inside an if/else
    # branch (lowered as a `let y = if … { … } else { … }`).
    if k > 0:
        y = table[k]
    else:
        y = 0
    return y
