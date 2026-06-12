/* PMAT-479 (R10): C early returns (guard clauses). A non-final return
   inside an if-branch lowers to Stmt::Return; the function still ends
   with a trailing return. Rust emits `return e;` + the trailing expr. */
int fact(int n) {
    if (n <= 1) {
        return 1;
    }
    return n * fact(n - 1);
}

int sign(int x) {
    if (x > 0) {
        return 1;
    }
    if (x < 0) {
        return -1;
    }
    return 0;
}
