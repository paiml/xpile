# PMAT-635: range-comprehension variables are scoped to the comprehension and
# must NOT leak into / clobber an enclosing binding (Python semantics). The
# desugar renames the counter to a fresh synthetic name.


# List comp whose variable shadows the parameter: the param must be unchanged.
def list_no_leak(i: int) -> int:
    xs = [i for i in range(3)]
    return i + len(xs)  # 99 + 3 = 102 (param i preserved)


# Dict comp shadowing the parameter.
def dict_no_leak(i: int) -> int:
    d = {i: i for i in range(3)}
    return i + len(d)  # 99 + 3 = 102


# Set comp shadowing the parameter.
def set_no_leak(i: int) -> int:
    s = {i for i in range(3)}
    return i + len(s)  # 99 + 3 = 102


# The comprehensions still compute their values correctly.
def comp_values() -> int:
    a = sum([k * k for k in range(4)])  # 0+1+4+9 = 14
    b = sum([k for k in range(0, 10, 2)])  # 0+2+4+6+8 = 20
    return a + b  # 34
