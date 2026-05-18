# PMAT-124 — Runtime witness for the Python ↔ C extension boundary.
#
# Provides a Runtime-stratum vote for:
#   C-FFI-CPYTHON-EXT
#
# Small Python module that calls into a CPython C extension — the
# canonical hybrid shape the contract pins down. The contract's
# load-bearing invariants are:
#   * manifest_completeness — every Python→C call site is
#     recorded in the FFI manifest
#   * refcount_balance_on_success — borrowed refs stay borrowed
#   * refcount_balance_on_error  — errors don't leak
#   * gil_invariant              — GIL held across the boundary
#   * buffer_protocol_zero_copy  — ndarray passes through O(1)
#
# Hybrid transpilation isn't yet wired at v0.1.0 — the only
# cross-domain hybrid that shipped is Python→shell via
# subprocess.run recognition (PMAT-040). The Python+C/NumPy
# hybrid demo is XPILE-HYBRID-NUMPY-001 future work; this
# fixture is in place ahead of that wiring so the contract can
# move from 3-stratum QUORUM to full 4-stratum.
#
# A dedicated round-trip test that lowers this hybrid module
# and asserts FFI manifest completeness + refcount balance is
# XPILE-FFI-CPYTHON-RUNTIME-001 future work.

import numpy as np


def compute_sum(xs: list[float]) -> float:
    # numpy is a CPython C extension — calling np.array() and
    # np.sum() crosses the Python→C boundary. The transpiled
    # output must preserve manifest completeness and refcount
    # balance.
    arr = np.array(xs, dtype=np.float64)
    return float(np.sum(arr))
