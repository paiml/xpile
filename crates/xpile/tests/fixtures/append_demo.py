# PMAT-460 / v0.2.0 Track 1.B: list.append() mutation. Exercises
# Stmt::ListAppend with parameter-mut threading. `xs` types as
# list[int] AND gets mutated → frontend marks param mutable →
# Rust/Ruchy emit `mut xs: Vec<i64>` so .push() type-checks.
# Governing contract: C-XLATE-PY-LIST-TO-VEC's alias_observation_inserts_clone
# Bronze theorem (single-owner mutation case).
def double_and_append(xs: list[int], n: int) -> int:
    xs.append(n)
    xs.append(n + n)
    return len(xs)
