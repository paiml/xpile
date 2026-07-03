def key_msg(k: str) -> str:
    # PMAT-1089: str(KeyError(k)) is repr(k) — a missing str key binds the
    # repr-QUOTED key ("'mk'", quote-switched for keys containing a quote),
    # not a fixed "key not found" text.
    d = {"a": 1}
    try:
        return "hit: " + str(d[k])
    except KeyError as e:
        return str(e)


def key_msg_int(k: int) -> str:
    # An int key binds its plain repr (no quotes).
    d = {1: 10}
    try:
        return "hit: " + str(d[k])
    except KeyError as e:
        return str(e)


def pop_msg(k: str) -> str:
    # d.pop(k) with no default raises the same repr-keyed KeyError.
    d = {"a": 1}
    try:
        return "hit: " + str(d.pop(k))
    except KeyError as e:
        return str(e)


def int_msg(s: str) -> str:
    # CPython: "invalid literal for int() with base 10: '<orig>'" — the
    # ORIGINAL (untrimmed) argument, repr-quoted. The Rust .expect() used to
    # leak "ParseIntError { kind: InvalidDigit }" into str(e).
    try:
        return "ok: " + str(int(s))
    except ValueError as e:
        return str(e)


def int16_msg(s: str) -> str:
    try:
        return "ok: " + str(int(s, 16))
    except ValueError as e:
        return str(e)


def float_msg(s: str) -> str:
    # CPython: "could not convert string to float: '<orig>'".
    try:
        return "ok: " + str(float(s))
    except ValueError as e:
        return str(e)
