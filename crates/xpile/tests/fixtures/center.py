# PMAT-502cu (Tranche 2): str.center(width) — space-pad centred, matching
# CPython's parity-dependent bias (left = marg/2 + (marg & width & 1)), so
# "ab".center(5) == "  ab " (not Rust {:^}'s " ab  "). Already-wide unchanged.
def c(s: str) -> str:
    return s.center(5)
