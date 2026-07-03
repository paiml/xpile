# PMAT-1058 (exceptions epic): STATEMENT-form `try: <stmts> except E [as e]:
# <stmts>` — arms are side-effecting statement blocks, neither producing a
# value. The value-form Expr::TryCatch covers `try: x = e except: x = e` /
# `try: return e except: return e`; the new Stmt::TryCatch covers the common
# `try: risky_call() except E: handle()` shape. Rust/Ruchy emit
# `match catch_unwind(|| { <body> }) { Ok(_) => {}, Err(...) => { <handler> } }`
# with the SAME PMAT-789 allowlist re-raise (an unlisted exception propagates,
# not swallowed) and PMAT-817 `as e` message binding. Lean/WASM refuse.
# A name FIRST-bound inside the try body and read AFTER the try refuses at
# lowering (PMAT-1092 — was rustc E0425; the value model can't express
# Python's leak of a maybe-unset try local) — set it in both arms (the
# assignment-form) or pre-declare it. Verified vs CPython (caught/oob/-1/boom/2/0).
def guard(n: int) -> int:
    if n < 0:
        raise ValueError("neg")
    return n * 2


def risky_div(n: int) -> int:
    return 10 // n


def catch_specific() -> str:
    result: str = "no"
    try:
        guard(-1)
    except ValueError:
        result = "caught"
    return result


def catch_index() -> str:
    xs: list[int] = [1, 2, 3]
    result: str = "no"
    try:
        _ = xs[10]
    except IndexError:
        result = "oob"
    return result


def catch_all() -> int:
    result: int = 0
    try:
        result = risky_div(0)
    except:
        result = -1
    return result


def bind_message() -> str:
    msg: str = "none"
    try:
        raise ValueError("boom")
    except ValueError as e:
        msg = e
    return msg


def multi_statement() -> int:
    x: int = 0
    try:
        x = 1
        raise ValueError("x")
    except ValueError:
        x = 2
    return x


def reraise_uncaught(n: int) -> int:
    result: int = -99
    try:
        result = 1 // n
    except ValueError:
        result = 0
    return result

