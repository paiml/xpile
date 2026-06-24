def main() -> None:
    # PMAT-923: the `#` alternate-form radix format spec — prefixed 0x/0o/0b,
    # sign first for negatives, and an uppercase `0X` prefix for `#X`.
    print(f"{255:#x}")
    print(f"{255:#X}")
    print(f"{255:#o}")
    print(f"{255:#b}")
    print(f"{-255:#x}")
    print(f"{-255:#X}")
    print(f"{0:#x}")
    print(f"{-8:#o}")
    print(f"{5:#b}")
    print(f"v={-255:#x}!")
