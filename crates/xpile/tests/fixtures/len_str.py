# PMAT-459 / v0.2.0 Track 1.B: builtin `len(s)` over a str. Returns
# the UTF-8 byte length (matching Rust's String::len semantics).
# Note: this is byte length, not codepoint count — Python's `len(s)`
# on a Python str returns codepoint count, but Python's bytes type
# returns byte length. v0.2.0 first cut chooses byte length to match
# C-XLATE-PY-STR-TO-RUST-STRING's utf8_bytes_preserved Bronze theorem;
# codepoint-count semantics is a Silver-tier refinement (v0.3.0+).
def name_len(s: str) -> int:
    return len(s)
