# PMAT-923 (correctness-hunt): the `#` ALTERNATE-FORM radix format spec
# (f"{255:#x}" == "0xff", "#X" == "0XFF", "#o" == "0o377", "#b" == "0b11111111")
# was a clean reject ("unsupported format spec `:#x`"). Python emits the same
# 0x/0o/0b prefix as the hex/oct/bin builtins, with the sign FIRST for negatives
# (f"{-255:#x}" == "-0xff"); `#X` also uppercases the prefix letter (0XFF). The
# spec now routes to the prefixed sign-magnitude IntRadixStr path. vs python3.


def hx(n: int) -> str:
    return f"{n:#x}"


def hX(n: int) -> str:
    return f"{n:#X}"


def oc(n: int) -> str:
    return f"{n:#o}"


def bi(n: int) -> str:
    return f"{n:#b}"


def labeled(n: int) -> str:
    return f"v={n:#x}!"
