# PMAT-492 (sprint): Python no-arg string transform methods.
# .upper()/.lower()/.strip() lower to Expr::StrMethod, which emits
# .to_uppercase()/.to_lowercase()/.trim().to_string() in Rust/Ruchy.
def shout(s: str) -> str:
    return s.upper()


def quiet(s: str) -> str:
    return s.lower()


def clean(s: str) -> str:
    return s.strip()
