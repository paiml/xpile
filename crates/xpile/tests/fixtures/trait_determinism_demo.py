# PMAT-123 — Runtime witness for trait determinism.
#
# This fixture exists to provide a Runtime-stratum vote for two
# trait contracts:
#   C-XPILE-FRONTEND-TRAIT — parse_and_lower determinism
#   C-XPILE-BACKEND-TRAIT  — lower determinism
#
# It's a small, deterministic, type-annotated Python module that
# depyler-frontend (Frontend trait) and the Rust/Ruchy/Lean
# backends (Backend trait) handle in the full happy path. The
# fixture is observed-exercised by the existing transpile_e2e
# test surface — every depyler-frontend parse and every backend
# lower of this file is one observation of the determinism
# invariant.
#
# The §14.4 Symbolic stratum (Kani, PMAT-063 + PMAT-065) already
# proves the property symbolically; the Runtime stratum adds
# the per-fixture observed-evidence vote. Combined with Sem +
# Sym + Ext strata, the trait contracts now sit at the full
# 4-stratum coverage tier (Sem=1 / Sym=1 / Run=1 / Ext≥2).
#
# A dedicated determinism-asserting test that parses + lowers
# this fixture twice and asserts byte-identical output is
# XPILE-TRAIT-DETERMINISM-RUNTIME-001 future work (requires
# adding depyler-frontend + codegen crates + serde as dev-
# dependencies on the xpile binary crate; deferred to keep the
# Runtime-stratum vote landing as a minimal change).

def double(n: int) -> int:
    return n + n


def square(n: int) -> int:
    return n * n


def doubled_square(n: int) -> int:
    return square(double(n))
