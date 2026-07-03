# PMAT-1172: PMAT-1170 (#1771) populated the `exc_bindings` side-channel only in
# `lower_statement_try`, so `repr(e)` in the TERMINAL-RETURN form
# (`try: return X except E as e: return repr(e)`) and the ASSIGN form
# (`v = <try/except with repr(e)>`) still emitted the bare message string
# (`'msg'`) instead of CPython's `<Type>('msg')`. This slice records the caught
# type at both of those binding sites too. `str(e)` is unaffected; the `repr`
# consumption site (site-agnostic, reuses `Expr::ReprStr`) already wraps once the
# binding is recorded. All values differential-checked vs python3.
def terminal_value_err(s: str) -> str:
    # terminal-return form: try body is a single `return`, and int(s) raises
    # ValueError on bad input. repr(e) == ValueError("<the int() message>").
    try:
        return str(int(s))
    except ValueError as e:
        return repr(e)


def terminal_key_err(k: str) -> str:
    # terminal-return form with KeyError — its str(e) is already repr(key), so
    # repr(e) is KeyError('<key>') without double-quoting.
    d = {"a": "x"}
    try:
        return d[k]
    except KeyError as e:
        return repr(e)


def assign_value_err(s: str) -> str:
    # assign form: both arms assign the same name; repr(e) in the handler value.
    try:
        v = str(int(s))
    except ValueError as e:
        v = repr(e)
    return v
