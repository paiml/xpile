# Hybrid Transpile Flow

**Section 16 of [xpile-spec.md](../xpile-spec.md).**

## The flow that no single-language transpiler can do

```text
$ xpile transpile --hybrid foo_module/

foo_module/
├── foo.py        (depyler-frontend)
├── _foo_core.c   (decy-frontend)
└── setup.py      (build manifest)

  ┌──────────────────────────────────────────────────────────────────┐
  │ Phase 1 — Dispatch                                               │
  │   xpile-core walks foo_module/ and dispatches each file to its   │
  │   frontend by extension                                           │
  │     foo.py        → depyler-frontend.parse_and_lower()           │
  │     _foo_core.c   → decy-frontend.parse_and_lower()              │
  │     setup.py      → depyler-frontend (build-config-aware mode)   │
  │   Result: Vec<Module> with per-Module ffi_boundaries populated   │
  ├──────────────────────────────────────────────────────────────────┤
  │ Phase 2 — FFI Manifest Reconciliation                            │
  │   xpile-ffi-manifest::reconcile(modules) walks all outgoing      │
  │   boundaries and pairs them with incoming exports:               │
  │     foo.py imports _foo_core.sum                                 │
  │     _foo_core.c exports PyObject* sum(PyObject* args)            │
  │     → manifest entry: sum(arr: ndarray<f64>) -> f64              │
  │       with convention=cpython, shim_id=<sha256>                  │
  │   Unresolved boundaries fail this phase.                         │
  ├──────────────────────────────────────────────────────────────────┤
  │ Phase 3 — Oracle Capture                                         │
  │   xpile-oracle runs the original Python module on the fixture:   │
  │     python3 -c "import foo_module; ... captures outputs"         │
  │   Captured outputs are immutable for the rest of the session.    │
  ├──────────────────────────────────────────────────────────────────┤
  │ Phase 4 — Rust Emission                                          │
  │   xpile-rust-codegen takes Vec<Module> + FfiManifest:            │
  │     • emits one .rs per Module                                   │
  │     • emits ffi_shims.rs (one shim per manifest entry)           │
  │     • emits Cargo.toml + (optional) build.rs                     │
  │   Generated workspace is rooted at target/transpiled/foo_module/ │
  ├──────────────────────────────────────────────────────────────────┤
  │ Phase 5 — Verify                                                 │
  │   cargo build --manifest-path target/transpiled/foo_module/...   │
  │   If build clean:                                                │
  │     cargo test --oracle  (runs the captured fixture)             │
  │   On success: session exits with ExitCode::Success               │
  │   On build/oracle failure: fall through to Phase 6 (if --repair) │
  ├──────────────────────────────────────────────────────────────────┤
  │ Phase 6 — Agent Repair (opt-in, --repair)                        │
  │   xpile-agent enters bounded loop:                               │
  │     - read cargo diagnostics                                     │
  │     - read oracle divergence                                     │
  │     - apply relevant skills                                      │
  │     - write corrected .rs                                        │
  │     - cargo_build + run_hybrid_oracle                            │
  │   Exit when both pass (Match) or budget exhausts.                │
  └──────────────────────────────────────────────────────────────────┘
```

## First hybrid demo: Python → shell (shipped at v0.1.0)

The first cross-domain hybrid that actually shipped is Python recognising `subprocess.run([...])` calls and lowering them through meta-HIR's `Stmt::Cmd` variant to POSIX shell via `bashrs-backend` — PMAT-040, integration test `transpile_python_subprocess_run_to_shell_via_bashrs_backend`. This wasn't the originally-planned first demo (Python+C / NumPy was — see below) but it shipped first because the bashrs merger (PMAT-037..058) produced the load-bearing cross-domain IR variants ahead of the Python+C / FFI manifest work.

The Python→shell hybrid is the *simpler* shape (one frontend cross-recognising another domain's IR variants); the Python+C / NumPy demo described below is the *load-bearing* shape (two real frontends, FFI manifest linking, refcount-aware boundary). Both are now in scope at v0.1.0+ — see [bashrs-merger.md](bashrs-merger.md) for the Python→shell side.

## Planned demo target: CPython C extension

A NumPy-style module:

```python
# foo_module/foo.py
import numpy as np
from foo_module._core import compute

def run(xs):
    arr = np.array(xs, dtype=np.float64)
    return compute(arr)
```

```c
// foo_module/_core.c — built into _core.so via setup.py
#include <Python.h>
#include <numpy/arrayobject.h>

static PyObject* compute(PyObject* self, PyObject* args) {
    PyArrayObject* arr;
    if (!PyArg_ParseTuple(args, "O!", &PyArray_Type, &arr)) return NULL;
    double* data = PyArray_DATA(arr);
    npy_intp n = PyArray_DIM(arr, 0);
    double sum = 0;
    for (npy_intp i = 0; i < n; i++) sum += data[i] * data[i];
    return PyFloat_FromDouble(sum);
}
// ... PyModule_Create ...
```

**xpile hybrid output:**

```
target/transpiled/foo_module/
├── Cargo.toml
├── src/
│   ├── lib.rs          # from foo.py
│   ├── core.rs         # from _core.c
│   └── ffi_shims.rs    # from manifest
└── tests/
    └── oracle.rs       # captured from CPython execution
```

The `Cargo.toml` depends on `pyo3`, `ndarray`, `numpy` (pyo3 binding). The `lib.rs` imports `core::compute` via the shim in `ffi_shims.rs`.

## Why a single transpiler can't do this

- **depyler alone** can't translate `_core.c` — it doesn't have a C frontend
- **decy alone** can't translate `foo.py` — it doesn't have a Python frontend
- Even with both transpilers running side-by-side in separate processes, they'd have to **guess** at the boundary semantics: how is the ndarray marshalled? Is the refcount balanced on the C side? What's the GIL state?

The FFI manifest is the **explicit reconciliation point** that makes both sides agree. That's the architectural payoff.

## Boundary cases covered in v1

| Case | Convention | Status |
|---|---|---|
| Python ↔ C (CPython API) | `cpython` | Phase 5 target |
| Python ↔ C++ (pybind11) | `pybind11` | Phase 7+ |
| Python ↔ CUDA (`@cuda.jit`) | `cuda_kernel` | Phase 7+ |
| Ruchy ↔ Python (data interop) | `pyo3` | Phase 6 |
| Pure Python (no hybrid) | n/a | Phase 1-4 |
