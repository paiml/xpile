# PMAT-701: capitalize()/title() titlecase the lead char of each word, not
# uppercase it — so a titlecase-EXPANDING scalar matches Python: ß.capitalize()
# is "Ss" (not "SS"), ﬂ at a word start titlecases to "Fl" (not "FL"). xpile
# derives titlecase from the uppercase expansion (std has no char::to_titlecase).
def cap(s: str) -> str:
    return s.capitalize()


def titl(s: str) -> str:
    return s.title()
