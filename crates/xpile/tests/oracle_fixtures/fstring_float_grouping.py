def main() -> None:
    # PMAT-940: thousands-grouped FLOAT with fixed precision — `,`/`_` separators,
    # integer part grouped by 3, sign first for negatives, `.dd` tail intact, int
    # coerced to float by the float-presentation spec, bool coerced to int→float.
    print(f"{1234567.89:,.2f}")
    print(f"{1234567.5:_.1f}")
    print(f"{1234.5:,f}")
    print(f"{-1234567.89:,.2f}")
    print(f"{-12.5:,.0f}")
    print(f"{0.5:,.2f}")
    print(f"{999.99:,.2f}")
    print(f"{1000.0:,.0f}")
    print(f"{1234567:,.2f}")
    print(f"{1234567890.123:,.3f}")
    print(f"bal={12345678.0:,.2f}!")
    print(f"{True:,.2f}")
