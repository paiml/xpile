# PMAT-696: a float set element / dict key lowers to HashSet<f64> / HashMap<f64,_>,
# which is invalid Rust (f64 is not Eq/Hash → E0277). xpile rejects this at
# lowering instead of emitting uncompilable Rust.
def f() -> int:
    xs: set[float] = {1.5, 2.5}
    return len(xs)
