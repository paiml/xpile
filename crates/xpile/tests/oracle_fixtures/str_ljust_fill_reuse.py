def main() -> None:
    # PMAT-934: the 2-arg fill form `s.ljust(w, c)` MOVES its receiver in
    # codegen (`let __s = (recv)`), but `LJust` was missing from
    # `str_method_moves_receiver`, so reusing `s` after the call failed rustc
    # with E0382 where CPython runs fine. The differential oracle compiles +
    # runs the transpiled module and byte-compares its stdout to CPython's.
    s: str = "ab"
    print(s.ljust(6, "."))
    print(s.ljust(6, "."), len(s))
    print(s.rjust(6, "*"), s.ljust(6, "-"), s)
