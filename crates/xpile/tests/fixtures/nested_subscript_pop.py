# PMAT-715: `xs[i].pop()` must mutate the inner container IN PLACE. The read path
# cloned `xs[i]` and popped the clone, so `xs[i]` kept its length (silent-wrong:
# the popped VALUE was correct but the side-effect was lost). Now emits an l-value
# pop over a `mut` base, with negative-index wrapping.
def pop_inner(xs: list[list[int]]) -> int:
    v = xs[0].pop()
    return v * 10 + len(xs[0])


def pop_neg(xs: list[list[int]]) -> int:
    v = xs[-1].pop()
    return v + len(xs[-1])


def plain_pop(xs: list[int]) -> int:
    v = xs.pop()
    return v + len(xs)
