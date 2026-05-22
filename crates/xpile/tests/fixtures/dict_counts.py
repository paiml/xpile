# PMAT-462 / v0.2.0 Track 1.C foundation: dict[str, int] literal.
# Exercises Type::Dict(Box<Type::Str>, Box<Type::I64>) + Expr::DictLit
# end-to-end. Rust emits a block expression returning an owned
# HashMap<String, i64>; Lean emits `List (String × Int)` first cut.
def counts() -> dict[str, int]:
    return {"alice": 1, "bob": 2, "carol": 3}
