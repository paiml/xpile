# PMAT-748 (HUNT-V14 #3 str-literal-control-byte-escape): a Python string
# literal containing control bytes must round-trip exactly. xpile emitted the
# raw bytes into the Rust `String::from("...")` literal: a bare CR (`"\r"`) is a
# hard rustc error ("bare CR not allowed in string"), and a raw CRLF was
# normalized by Rust's lexer to a lone LF — silently DROPPING the CR (wrong
# `len`, wrong bytes for protocol/Windows strings). Control bytes are now
# escaped (`\r`/`\n`/`\t`/`\0`/`\u{..}`) in both plain literals and f-string
# literal segments. Cross-checked vs python3.


def cr_len() -> int:
    # bare CR used to be a rustc compile error
    return len("data\r")


def crlf_len() -> int:
    # raw CRLF used to be normalized to LF (CR dropped) → len 3 not 4
    return len("x\r\ny")


def tab_len() -> int:
    return len("a\tb")


def nul_len() -> int:
    return len("a\x00b")


def esc_in_fstring_len(n: int) -> int:
    # control bytes in an f-string literal segment
    return len(f"row\r\n{n}")
