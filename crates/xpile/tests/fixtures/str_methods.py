# PMAT-492 (sprint): Python no-arg string transform methods +
# PMAT-493b: startswith/endswith predicates.
# upper/lower/strip lower to Expr::StrMethod (Str); startswith/endswith
# carry a pattern arg and lower to Rust .starts_with/.ends_with (Bool).
def shout(s: str) -> str:
    return s.upper()


def quiet(s: str) -> str:
    return s.lower()


def clean(s: str) -> str:
    return s.strip()


def is_greeting(s: str) -> bool:
    return s.startswith("hello")


def is_question(s: str) -> bool:
    return s.endswith("?")


def words(s: str) -> list[str]:
    return s.split(" ")


def joined(xs: list[str]) -> str:
    return " ".join(xs)
