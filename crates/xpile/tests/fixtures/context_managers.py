# PMAT-1072: user CONTEXT MANAGERS — `with ClassName(...) [as x]: BODY` over a
# class defining __enter__/__exit__. AST pre-pass desugars to `__cm =
# ClassName(...); x = __cm.__enter__(); try: BODY finally: __cm.__exit__(<zero
# args>)` — a finally-only try (PMAT-1073) so __exit__ runs on the exception
# path too. Refuses precisely: `with open(...)` (file I/O), non-constructor /
# multi-item forms, and MUTATING context managers (whose __enter__/__exit__
# mutate self — a captured mutable would be cloned and the mutation silently
# dropped, the Rc<RefCell> reference-model gap). Verified vs CPython.
class Resource:
    v: int

    def __init__(self, v: int) -> None:
        self.v = v

    def __enter__(self) -> "Resource":
        return self

    def __exit__(self, a: int, b: int, c: int) -> None:
        pass

    def doubled(self) -> int:
        return self.v * 2


class Tracer:
    def __enter__(self) -> "Tracer":
        print("acquire")
        return self

    def __exit__(self, a: int, b: int, c: int) -> None:
        print("release")


def use_resource() -> int:
    result: int = 0
    with Resource(21) as r:
        result = r.doubled()
    return result


def trace_lifecycle() -> None:
    with Tracer():
        print("work")


def exit_runs_on_exception() -> str:
    outcome: str = "?"
    try:
        with Tracer():
            raise ValueError("boom")
    except ValueError:
        outcome = "caught"
    return outcome
