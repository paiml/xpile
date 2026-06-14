# PMAT-609: list.pop(i) with a RUNTIME negative index. Python pop(i) with i<0
# removes from the end (i+len); a bare `(i) as usize` cast wraps a negative i to
# usize::MAX -> Vec::remove panics. The runtime index is now normalized
# (i<0 -> i+len). Literal pop(-1)/pop(0) keep their existing lowering.
def pop_at(xs: list[int], i: int) -> int:
    return xs.pop(i)
