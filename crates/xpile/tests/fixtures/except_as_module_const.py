# PMAT-1105 (d) (skeptic pass PMAT-1098, C-F4): an `except ... as E` that
# collides with a MODULE-LEVEL constant is refused with the GLOBAL scoping
# truth — `except as` localizes `E` for the whole function scope (the module
# `E` is shadowed and survives the handler-exit implicit del; a post-handler
# read raises UnboundLocalError unconditionally) — not the live-local
# rationale, whose both assertions are false for this case.
E = 5


def f() -> int:
    r = 0
    try:
        print("body")
        r = int("x")
    except ValueError as E:
        print("handler")
        r = 1
    return r
