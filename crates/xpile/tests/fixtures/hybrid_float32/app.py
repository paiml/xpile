# #2139: a Python call into a C `float` boundary must build and agree with
# CPython under plain `xpile hybrid --verify`.
#
# Until #2139, `--verify` skipped this boundary as "non-ABI-mappable" and
# exited 0, while `--emit-workspace` emitted `twice(1.1f64)` into an `f32`
# wrapper (E0308). The hybrid workspace now bridges it with
# `fn twice(a0: f64) -> f64 { ffi_shims::twice_shim(a0 as f32) as f64 }`,
# which rounds and widens exactly as ctypes' `c_float` binding does. The lines
# below pin where f32 rounding shows in the printed repr (1.1, 0.1), an exact
# value (3.0), and a result used in f64 arithmetic.
from ._core import twice


def main() -> None:
    print(twice(1.1))
    print(twice(3.0))
    print(twice(0.1))
    y = twice(2.5)
    print(y + 1.0)
