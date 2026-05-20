# PMAT-452 / v0.2.0 Track 1.A: f-string lowering — the canonical
# v0.2.0 EXIT CRITERION fixture from sub/v0.2.0-depyler-merger.md.
# f"Hello, {name}!" lowers to chained Expr::Concat which emits
# nested format!() in Rust and ++ in Lean. Conversion flags (!r/!s)
# and format specs (:>10) are not supported at v0.2.0 first cut.
def greet(name: str) -> str:
    return f"Hello, {name}!"
