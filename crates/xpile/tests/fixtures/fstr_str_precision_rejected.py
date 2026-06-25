# PMAT-947 (correctness-hunt): a WIDTH-COMBINED str precision `:10.3` is a scoped
# follow-up — Python pads the truncated string to the width (f"{'hello':10.3}" ==
# "hel       "), which is also sound in Rust, but it is deferred behind the bare
# `.N` slice (mirroring PMAT-945 -> PMAT-946). It must refuse honestly rather than
# miscompile. vs python3.
def wp(s: str) -> str:
    return f"{s:10.3}"
