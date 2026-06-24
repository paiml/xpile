def main() -> None:
    # PMAT-906: a `: float` annotation is a non-enforcing assertion, not a
    # runtime cast. Python keeps the int value (`print` shows "5", not "5.0");
    # xpile must match, not silently widen to f64 (silent-wrong) or emit
    # `let a: f64 = <i64>` (rustc E0308).
    a: float = 5
    print(a)
    n: int = 7
    b: float = n
    print(b)
    # An int-typed expression under a float annotation stays int, too.
    d: float = a + n
    print(d)
    print(b * 2)
    # A genuinely-float initializer is unaffected — still a real f64.
    c: float = 3.5
    print(c)
    e: float = c
    print(e)
    f: float = 10 / 4
    print(f)
