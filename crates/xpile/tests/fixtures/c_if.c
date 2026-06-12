/* PMAT-478 (R9): C if/else statements (Stmt::If). Branch bodies do
   assignments (no early return — that is R10); locals reassigned in a
   branch are inferred `mut`. */
int max3(int a, int b, int c) {
    int m = a;
    if (b > m) {
        m = b;
    }
    if (c > m) {
        m = c;
    }
    return m;
}

int clamp(int x, int lo, int hi) {
    int r = x;
    if (x < lo) {
        r = lo;
    } else {
        if (x > hi) {
            r = hi;
        }
    }
    return r;
}
