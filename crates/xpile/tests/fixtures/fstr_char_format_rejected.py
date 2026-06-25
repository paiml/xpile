# PMAT-945 (correctness-hunt): a `:c` char-format spec is INT-ONLY in Python —
# applying it to a FLOAT (or str) raises ValueError ("Unknown format code 'c'").
# xpile only routes `:c` to chr for an I64 value; a float `:c` must therefore stay
# a clean reject (an honest refusal mirroring Python's own error), never a silent
# miscompile. vs python3.


def bad(x: float) -> str:
    return f"{x:c}"
