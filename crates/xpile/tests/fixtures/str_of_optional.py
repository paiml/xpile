# PMAT-857 (HUNT-V28 #1): str() over an Optional — str(None), str(d.get(k)) — was
# a lowering reject ("body produces I64") because the str() builtin had no Option
# arm. It now lowers to `if opt.is_none() { "None" } else { <str of opt.unwrap()> }`
# (bare None → "None" directly). Cross-checked vs python3.


def from_none_lit() -> str:
    return str(None)


def get_int_absent(d: dict[str, int]) -> str:
    return "v=" + str(d.get("b"))


def get_int_present(d: dict[str, int]) -> str:
    return str(d.get("a"))


def get_str(d: dict[str, str]) -> str:
    return str(d.get("a"))


def get_float(d: dict[str, float]) -> str:
    return str(d.get("a"))
