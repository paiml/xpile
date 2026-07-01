# PMAT-1014 (sweep #7): the four Latin DIGRAPH triples (Ǆǅǆ / Ǉǈǉ / Ǌǋǌ /
# Ǳǲǳ) have a distinct TITLECASE (Lt) middle form — Python's capitalize()/
# title() map ANY of the three forms to it ('ǳ'.capitalize() == 'ǲ'). The
# PMAT-701 uppercase-expansion derivation gave the all-caps first form
# (ǳ→Ǳ, U+01F1) instead of ǲ (U+01F2): a silent DIVERGE. A const range-match
# now intercepts the digraphs before the expansion; ß→Ss / ﬂ→Fl expansion and
# ASCII behavior are unchanged.
def cap(s: str) -> str:
    return s.capitalize()


def titl(s: str) -> str:
    return s.title()
