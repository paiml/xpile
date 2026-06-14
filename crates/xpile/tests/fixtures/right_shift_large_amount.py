def sh(x: int, n: int) -> int:
    # Python defines `x >> n` for any non-negative n: once n reaches the bit
    # width the result saturates to the sign fill (0 for x >= 0, -1 for x < 0).
    # Rust's `checked_shr` returns None for n >= 64, so the emitted `.expect`
    # previously PANICKED where Python returns a value.
    return x >> n
