# PMAT-1352: the DIVERGENT twin of `hybrid_sum` — the fixture that makes
# `xpile hybrid --verify` take its ComparisonResult::Divergence arm.
#
# The FFI boundary here is deliberately IDENTICAL to hybrid_sum's and is
# expected to agree: `square_sum(7)` must print 49 on both sides. That is the
# point. An FFI *mis-cast* cannot produce a divergence at all, because
# `ctypes_binding_for` (the CPython reference's binding) and the emitted shim
# are both derived from the SAME meta-HIR types — so whatever they get wrong,
# they get wrong identically. Both sides here really do call one cc-compiled
# `int square_sum(int)` through a c_int binding.
#
# The divergence is on the PYTHON side, and it is a documented, still-open one:
# CHANGELOG "Known divergences" item 5 (OPEN POLICY — int literals in
# float-annotated containers). Python does not enforce annotations, so CPython
# keeps the int and prints `[1, 2.5]`; xpile treats the annotation as a
# coercion instruction and prints `[1.0, 2.5]`.
#
# COUPLING, on purpose: if that owner decision is ever made — annotation as a
# checked assertion rather than a coercion instruction — this fixture stops
# diverging and the test that drives it fails LOUDLY, which is the correct
# prompt to re-point it at whatever the then-current divergence is. It is not a
# test of the coercion behaviour; it is a carrier for exercising the arm.
from ._core import square_sum


def main() -> None:
    xs: list[float] = [1, 2.5]
    # Line 1 of stdout AGREES — proves the differential is not simply failing
    # everything, and puts the divergence on a non-zero line index so the
    # reported line number is meaningful.
    print(square_sum(7))
    # Line 2 DIVERGES: CPython `[1, 2.5]` vs artifact `[1.0, 2.5]`.
    print(xs)
