# PMAT-502aj (Tranche 2): s.title() -> title-case each word (first alpha of
# each word upper, rest lower; any non-alpha is a word boundary), matching
# Python exactly (incl. "it's".title() -> "It'S").
def t(s: str) -> str:
    return s.title()
