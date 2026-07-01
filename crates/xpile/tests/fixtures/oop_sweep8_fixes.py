# PMAT-1017 (sweep #8 on the day-old PMAT-1016 OOP surface): four fix classes.
# (1) EXPRESSION-position mutating calls — `a = p.take()`, `c.next() + c.next()`,
#     print args — left the receiver non-mut (rustc E0596 ×4 findings, one root
#     cause: the call-site walker scanned statement position only; now mirrors
#     count_pop_receivers' statement+expression recursion).
# (2) FieldAssign coerces to the FIELD's declared type — `self.x = n` (int param,
#     float field) emitted `C { x: n }` E0308; now widens via to_f64_operand.
# (3) len(<field>) in a comparison emitted `…count() as i64 < 6` — rustc parses
#     `i64<` as GENERICS; CharCount now parenthesizes.
# (4) struct-alias refinements: a REBOUND source (`c = Counter(99)` detaches the
#     name — not object mutation) and a DEAD alias are clone-safe, not refusals.
class Counter:
    count: int

    def __init__(self, start: int) -> None:
        self.count = start

    def take(self) -> int:
        self.count = self.count - 1
        return self.count

    def value(self) -> int:
        return self.count


class Gauge:
    s: str
    x: float

    def __init__(self, s: str, n: int) -> None:
        self.s = s
        self.x = n

    def short(self) -> bool:
        return len(self.s) < 6


def expr_position_mut() -> int:
    p = Counter(5)
    a = p.take()
    b = p.take() + p.take()
    return a + b + p.value()


def rebound_alias() -> int:
    c = Counter(5)
    c2 = c
    c = Counter(99)
    return c.value() + c2.value()


def field_widen_and_len(n: int) -> float:
    g = Gauge("ab", n)
    if g.short():
        return g.x + 0.5
    return g.x
