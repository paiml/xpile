def main() -> None:
    # PMAT-939: the thousands-grouping format spec — `,` and `_` separators,
    # grouped by 3 from the right, sign first for negatives, bool coerced to int.
    print(f"{1000000:,}")
    print(f"{1000000:_}")
    print(f"{1234567:,}")
    print(f"{-1234567:,}")
    print(f"{-1234567:_}")
    print(f"{0:,}")
    print(f"{100:,}")
    print(f"{1000:_}")
    print(f"{999:,}")
    print(f"total={12345678:,}!")
    print(f"{True:,}")
