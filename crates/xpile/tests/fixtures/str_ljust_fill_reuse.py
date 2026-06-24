# PMAT-934 (HUNT differential STR-LJUST-FILL-MOVE): the 2-arg fill form
# `s.ljust(w, c)` lowers to codegen that binds `let __s = (recv)` — MOVING the
# receiver, exactly like `rjust`/`center`/`zfill`. But `LJust` was missing from
# `str_method_moves_receiver`, so a reused source variable after a 2-arg `ljust`
# was a use-after-move (rustc E0382), where CPython runs fine. (`RJust` was
# already covered; `LJust` was the asymmetric gap.) The 1-arg form
# (`s.ljust(w)`) borrows via `format!("{:<1$}", s, w)` and never needed a clone;
# the now-included clone is gated on `read_count > 1`, so single-use emission is
# unchanged. Cross-checked vs python3.


def ljust_fill_reuse(s: str) -> int:
    padded = s.ljust(6, ".")
    return len(padded) + len(s)  # 6 + len(s)


def ljust_one_arg_reuse(s: str) -> int:
    # 1-arg ljust borrows the receiver (format width); reuse was always fine.
    padded = s.ljust(6)
    return len(padded) + len(s)  # 6 + len(s)


def ljust_fill_single_use(s: str) -> int:
    # not reused → no clone (unchanged emission).
    return len(s.ljust(6, "."))
