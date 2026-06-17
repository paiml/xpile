# PMAT-756 (HUNT-V15 #9 STR-MOVE-RECEIVER): str methods whose codegen binds
# `let __s = (recv)` (zfill/center/rjust/removeprefix/removesuffix/count/find/
# rfind/rindex/splitlines) MOVE the receiver, so reusing the source variable
# after the call was a use-after-move (rustc E0382). The receiver is now cloned
# when it's a reused non-Copy variable; a single use and borrow-only methods
# (.upper()/.startswith()) are unchanged. Cross-checked vs python3.


def pad_and_len(s: str) -> int:
    padded = s.zfill(8)
    return len(padded) + len(s)  # 8 + len(s)


def strip_prefix_reuse(s: str) -> int:
    short = s.removeprefix("foo")
    return len(short) + len(s)


def count_reuse(s: str) -> int:
    c = s.count("a")
    return c + len(s)


def single_use(s: str) -> int:
    # not reused → no clone (unchanged emission)
    return len(s.zfill(8))
