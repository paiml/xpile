# PMAT-502bw (Tranche 2): the `print` builtin → `println!`. Single-space
# separator, trailing newline; bare `print()` is a blank line; f-strings
# (which lower to String) print fine. int/str args only at first cut.
def demo(name: str, n: int) -> None:
    print("hello")
    print(n)
    print(name, n)
    print()
    print(f"{name}={n}")
