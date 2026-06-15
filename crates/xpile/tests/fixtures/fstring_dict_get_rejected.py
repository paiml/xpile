# PMAT-620: a no-default d.get(k) in an f-string field is Option<T>, which has
# no Display — f"{d.get(k)}" emitted format!("{}", Option) (E0308). str()/print()
# of a bare Optional already reject; the f-string case now rejects too (fail-loud)
# instead of emitting uncompilable Rust. Use d.get(k, default) or d[k] instead.
def f(d: dict[str, int], k: str) -> str:
    return f"val={d.get(k)}"
