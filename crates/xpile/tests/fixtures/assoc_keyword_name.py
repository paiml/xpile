# PMAT-823 (HUNT-V24 PROP-CM-02): a @staticmethod/@classmethod named after a Rust
# keyword had its definition escaped (PMAT-813) but the qualified call site
# Class::method left the keyword unescaped → rustc keyword error / E0425. The
# escape pass now escapes the method segment of a Class::method callee, so the
# call (Reg::r#match) agrees with the def. Cross-checked vs python3.
from dataclasses import dataclass


@dataclass
class Reg:
    n: int

    @staticmethod
    def match(x: int) -> int:
        return x * 2


def probe() -> int:
    return Reg.match(21)
