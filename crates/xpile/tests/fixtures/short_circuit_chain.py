# PMAT-638: short-circuit CHAINS `a or b or c` / `a and b and c` return the first
# decisive operand (Python truthiness). Leading operands are variables; the last
# may be any expression (a literal default).
def first_truthy(a: int, b: int, c: int) -> int:
    return a or b or c  # first non-zero, else c


def all_required(a: int, b: int, c: int) -> int:
    return a and b and c  # first zero, else c


def name_with_default(name: str, env: str) -> str:
    return name or env or "default"  # first non-empty, else "default"
