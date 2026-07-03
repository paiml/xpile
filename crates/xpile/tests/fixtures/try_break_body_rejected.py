# PMAT-1082 (skeptic-pass find, PMAT-1081 probes p04/p24): `break` (and
# `continue`) inside a statement-form try BODY has no enclosing loop inside
# the catch_unwind closure — rustc E0267, loud but far downstream. Now
# refused at lowering with a precise message.
def find_stop(n: int) -> int:
    total: int = 0
    for i in range(n):
        try:
            if i == 3:
                break
            total = total + i
        except ValueError:
            total = -1
    return total
