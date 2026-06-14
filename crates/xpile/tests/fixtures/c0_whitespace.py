# PMAT-600: Python treats the C0 information separators FS/GS/RS/US
# (U+001C..U+001F) as whitespace for isspace() and strip()/lstrip()/rstrip();
# Rust's char::is_whitespace() (and trim()) excludes them. The predicate now
# augments the Rust whitespace set with that range.
def is_ws(s: str) -> bool:
    return s.isspace()


def stripped(s: str) -> str:
    return s.strip()


def lstripped(s: str) -> str:
    return s.lstrip()


def rstripped(s: str) -> str:
    return s.rstrip()
