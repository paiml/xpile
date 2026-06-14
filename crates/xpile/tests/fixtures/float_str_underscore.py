# PMAT-611: float(str) accepts PEP 515 underscore digit separators
# (float("1_000.5") == 1000.5), which Rust's parse::<f64>() rejects → panic.
# Python allows underscores only BETWEEN digits (covering the fractional and
# exponent parts); xpile validates that, strips, then parses.
def parse(s: str) -> float:
    return float(s)


# A string-LITERAL argument transpiles to a temporary `String`; the validation
# block must keep it alive (temporary lifetime extension), not drop it (E0716).
def parse_literal() -> float:
    return float("1_234.5")
