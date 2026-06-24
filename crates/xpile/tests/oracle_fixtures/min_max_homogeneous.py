def main() -> None:
    # PMAT-917: homogeneous-numeric min/max stays fully supported and
    # byte-for-byte matches CPython. The mixed int+float forms (which Python
    # returns with the WINNING operand's own type, e.g. max(3, 2.5) == int 3)
    # are cleanly rejected at lowering — see depyler-frontend's BM-01 reject —
    # so they cannot reach this oracle. These all-int / all-float cases are the
    # supported, correct surface this fixture pins.
    print(max(3, 7, 2))
    print(min(3, 7, 2))
    print(max(-4, -9, -1))
    print(min(-4, -9, -1))
    print(max(3.5, 7.5, 2.5))
    print(min(3.5, 7.5, 2.5))
    print(max(1.0, 1.0))
    print(min(2.25, 2.25, 9.0))
