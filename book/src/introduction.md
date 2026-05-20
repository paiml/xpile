# xpile

**A polyglot transpile workbench with provable contracts at every layer.**

xpile is a CLI + library that takes a source file in one language and
emits an equivalent program in another, with the *equivalence* itself
pinned down as a machine-checked contract.

Seven frontends — Python, C, C++, Rust, Ruchy, Lean 4, Shell — share a
single canonical **meta-HIR** and dispatch through seven backends —
Rust, Ruchy, PTX, WGSL, SPIR-V, Lean 4, Shell. A **proof lane** parallel
to the code lane round-trips between LaTeX, Lean 4 theorems, and mdBook
through the same YAML contract substrate.

The repository ships with 12 contracts at full quorum (`pv lint` PASS, 0
errors), 638 stratum-vote artifacts (Lean theorems + Kani BMC harnesses),
and eleven UNIVERSAL Diamond milestones — every contract has paired
algebraic theorems at depths 3 through 13.

## Why xpile exists

Most transpilers live in a single repo per source-target pair: depyler
for Python→Rust, decy for C→Rust, and so on. That topology hits a wall
the moment you need **hybrid transpilation** — a single artifact that
crosses a language boundary:

- CPython program calling a C extension
- Python kernel launching CUDA code via PTX
- Python orchestrator shelling out to a POSIX script
- Rust crate invoking a Lean-derived correctness proof

xpile's premise: a shared meta-HIR + a shared contract substrate makes
those hybrid flows tractable. The same C-PY-INT-ARITH contract that
governs the Python-int → Rust-i64 overflow lane also governs the
Python-int → Lean-Int proof-lane shadow.

## What you can do at v0.1.0

- `xpile transpile factorial.py` → emit Rust with overflow checks
- `xpile transpile factorial.py --target ruchy` → emit Ruchy
- `xpile transpile factorial.py --target lean` → emit a Lean 4 `def`
- `xpile transpile script.sh --target shell` → POSIX-shell round-trip
- `xpile info`, `xpile diamond`, `xpile quorum` — inspect the substrate

## How this book is organised

- **Getting started** walks you through install + a first transpile.
- **Concepts** explains the two lanes, the contract taxonomy, and the
  Diamond-tier substrate.
- **Tutorials** are end-to-end recipes for common flows.
- **Reference** is exhaustive CLI / frontend / backend documentation.
- **Contributing** covers adding a frontend or backend.

Every concept page links back to the governing contract YAML so you can
trace a sentence in prose to the equation in `contracts/` to the Lean
theorem in `contracts/lean/` to the Kani harness in `contracts/kani/`.
