# A fitted linear-regression model, emitted as *proven code*.
#
# The coefficients below are constants baked from a fit; `predict` is a pure
# function over them. `xpile transpile model.py --emit-crate <dir>` turns this
# into a buildable Rust crate whose emitted `predict` carries its
# `// xpile-contract: C-PY-FLOAT-ARITH` citation — then
# `cargo build --target wasm32-wasip1` yields ONE portable `.wasm` that runs
# identically on any OS/arch under a WASI runtime.
def predict(sqft: float, bedrooms: float, age: float) -> float:
    return 0.115 * sqft + 15.2 * bedrooms + -0.9 * age + 32.5


def main() -> None:
    print(predict(1200.0, 3.0, 10.0))
    print(predict(2400.0, 4.0, 2.0))
    print(predict(800.0, 2.0, 30.0))
