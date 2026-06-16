# PMAT-710: a bool operand into a float-coercing op (/ // % **) emitted
# `(bool) as f64` → rustc E0606. Python's bool is an int subtype (True/2 == 0.5),
# so xpile now casts through i64: `((b) as i64) as f64`. One to_f64_operand fix
# covers all four operators.
def div(a: bool, b: int) -> float:
    return a / b


def floordiv(a: bool, b: float) -> float:
    return a // b


def modulo(a: bool, b: float) -> float:
    return a % b


def power(a: bool, b: float) -> float:
    return a ** b
