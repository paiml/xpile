# PMAT-844 (HUNT-V27 #4): d.get(k, 0) against a dict[_, float] emitted
# unwrap_or(0i64) on an Option<f64> → rustc E0308. DictGetOr types as the dict's
# value type (f64), so an int default must coerce to f64 (Python promotes 0→0.0
# in the float arithmetic anyway). An int-valued dict default stays i64 (the
# classic d.get(w,0)+1 counter is unaffected). Cross-checked vs python3.


def float_accum(vals: list[float]) -> float:
    d: dict[str, float] = {}
    for v in vals:
        d["s"] = d.get("s", 0) + v
    return d["s"]


def int_counter(words: list[str]) -> int:
    d: dict[str, int] = {}
    for w in words:
        d[w] = d.get(w, 0) + 1
    return d["a"] * 10 + d["b"]
