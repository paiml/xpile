# PMAT-449 / v0.2.0 Track 1.A: minimal Python `str` literal returning
# function — the foundation case for the depyler merger's string lane.
# `greet()` takes no args and returns a fixed literal so the test
# exercises only the new `Type::Str` + `Expr::LitStr` end-to-end path.
# Parameterised string fns + concatenation + f-strings are subsequent
# sub-track work (per sub/v0.2.0-depyler-merger.md sequencing).
def greet() -> str:
    return "hello"
