def describe(x: int) -> str:
    # A bare `type(x)` (reflective type object) has no Rust counterpart — must be
    # rejected with a clear message, not emit uncompilable `r#type(x)`.
    return str(type(x))
