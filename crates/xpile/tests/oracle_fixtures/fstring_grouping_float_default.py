def main() -> None:
    # PMAT-982: the BARE thousands-grouping spec `:,` / `:_` over a float's
    # DEFAULT repr (no `f` presentation) — group the integer part of str(float),
    # sign first, fractional / scientific / non-finite tail untouched.
    print(f"{1234567.5:,}")
    print(f"{1234.5:_}")
    print(f"{1000000.0:,}")
    print(f"{-1234567.5:,}")
    print(f"{0.1:,}")
    print(f"{100.0:,}")
    print(f"{12345.0:,}")
    print(f"{1234567890123.0:_}")
    print(f"{1e16:,}")
    print(f"bal={12345678.0:,}!")
