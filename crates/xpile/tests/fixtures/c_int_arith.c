/* PMAT-467 (v0.2.0 Track 2.A): the decy C → Rust exit criterion.
   Stack-only int subset: params, recursion, ternary, arithmetic.
   C arithmetic semantics (i32 + wrapping) — distinct from Python's
   i64 + checked. */
int add(int a, int b) { return a + b; }

int factorial(int n) { return n <= 1 ? 1 : n * factorial(n - 1); }

int poly(int x) {
    int sq = x * x;
    int lin = 2 * x + 1;
    return sq + lin;
}
