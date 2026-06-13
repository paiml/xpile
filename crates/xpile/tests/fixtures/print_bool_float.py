# PMAT-502bx (Tranche 2): print of bool/float args. Python prints `True`/
# `False` (capitalised) and floats with a `.0` on whole numbers (`3.0`),
# unlike Rust's `Display`. Reuses the str(bool)/str(float) machinery.
def demo(f: float, b: bool, n: int) -> None:
    print(f)
    print(b)
    print(3.0)
    print(n, f, b)
    print(2.5, "items")
