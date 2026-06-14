# PMAT-610: int(str) accepts PEP 515 underscore digit separators (int("1_000")
# == 1000), which Rust's parse::<i64>() rejects → panic. Python allows a single
# underscore only BETWEEN digits; xpile validates that, strips, then parses.
def parse(s: str) -> int:
    return int(s)


# PMAT-611: a string-LITERAL argument is a temporary `String`; the validation
# block must keep it alive (temporary lifetime extension), not drop it (E0716).
def parse_literal() -> int:
    return int("1_234")
