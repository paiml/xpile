# PMAT-813 (HUNT-V22 DEC-1): a method named after a Rust keyword (type/match/
# ref/move) emitted `pub fn type(&self)` and a call `(r).type()` verbatim — a
# rustc keyword parse error. The reserved-ident escape now escapes the method
# name at the definition AND the Expr::MethodCall callee (including an internal
# self-call), consistently. Builtin/dunder method names aren't keywords, so they
# are untouched. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Reg:
    n: int

    def type(self) -> int:
        return self.n * 2

    def match(self, x: int) -> int:
        return self.n + x

    def combined(self) -> int:
        return self.type() + self.match(3)


def probe() -> int:
    r = Reg(5)
    return r.type() + r.match(3) + r.combined()
