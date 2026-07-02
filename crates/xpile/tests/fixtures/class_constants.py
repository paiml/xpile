# PMAT-1054: CLASS CONSTANTS — a class-body `NAME: T = <literal>` used as a
# shared class attribute (`Config.VERSION`), NOT a per-instance field. Before
# this slice xpile treated every class-body annotated name as an instance
# struct field, so the three shapes below all failed (PMAT-1053):
#   (a) `Config.VERSION` (read via the CLASS NAME) — "reads attribute of a
#       non-struct value";
#   (b) `c.MAX` (instance read of a class const) — E0061 (`Config()` arity);
#   (c) a const alongside instance fields (`MAX`/`VERSION` + `n`) —
#       "`__init__` never assigns declared field MAX".
# A class-body default never assigned via `self` in a non-@dataclass class is a
# class constant: dropped from the struct's fields (so the ctor takes only `n`),
# reads fold to the literal. Differentially verified vs CPython (1.0/100/300/7).
class Config:
    VERSION: str = "1.0"
    MAX: int = 100

    def __init__(self, n: int) -> None:
        self.n = n

    def scaled(self) -> int:
        # `self.CONST` read inside a method resolves to the constant literal.
        return self.n * self.MAX


def version() -> str:
    # (a) ClassName.CONST — the receiver is a type, not a value.
    return Config.VERSION


def instance_max() -> int:
    # (b) instance.CONST — MAX is an associated constant, not a field.
    c: Config = Config(3)
    return c.MAX


def scaled_val() -> int:
    c: Config = Config(3)
    return c.scaled()


def field_val() -> int:
    # (c) a genuine instance field still constructs + reads normally.
    c: Config = Config(7)
    return c.n
