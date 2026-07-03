# PMAT-1096 (skeptic pass PMAT-1090 D-1/D-2): chr() range semantics.
# CPython 3.10 ground truth (verified via python3):
#   chr(n) for n outside C-int [-2**31, 2**31-1]  -> OverflowError
#           "Python int too large to convert to C int"
#   chr(n) for n in C-int but not in range(0x110000) -> ValueError
#           "chr() arg not in range(0x110000)"
#   chr(n) for n in the surrogate band 0xD800..0xDFFF -> SUCCEEDS in CPython;
#           the rust/ruchy lanes panic UNTYPED with an honest lane-limitation
#           payload (Rust char excludes surrogates) — a typed except must NOT
#           catch it (no Python exception corresponds).
# Before: the unchecked `as u32` cast WRAPPED — chr(-4294967295) silently
# yielded "\x01" — and the panic payload claimed "not in range(0x110000)"
# for in-range surrogates.
def chr_ok(n: int) -> str:
    return chr(n)


def chr_val_msg(n: int) -> str:
    # in-C-int out-of-range binds the exact CPython ValueError message
    # (the old `(ValueError)` SUFFIX payload was never typed-catchable).
    try:
        return "ok: " + chr(n)
    except ValueError as e:
        return str(e)


def chr_ovf_msg(n: int) -> str:
    # outside C-int binds the exact CPython OverflowError message.
    try:
        return "ok: " + chr(n)
    except OverflowError as e:
        return str(e)
