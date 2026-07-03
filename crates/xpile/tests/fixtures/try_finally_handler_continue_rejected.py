# PMAT-1082: with a `finally`, the every-exit-path wrap (PMAT-1070) puts the
# `except` HANDLER inside the OUTER catch_unwind closure too — so `continue`
# there cannot reach the loop (rustc E0267). Refused at lowering; without the
# finally the handler runs at loop scope and handler-continue is supported
# (see try_handler_continue.py).
def f(n: int) -> int:
    total: int = 0
    cleanup: int = 0
    for i in range(n):
        try:
            if i == 1:
                raise ValueError("bad")
            total = total + i
        except ValueError:
            continue
        finally:
            cleanup = cleanup + 1
    return total + cleanup
