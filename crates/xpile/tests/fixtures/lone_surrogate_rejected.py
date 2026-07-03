# PMAT-1086: a lone-surrogate escape decodes lossily to U+FFFD in the
# emitted program (Rust String / the UTF-8 WASM ABI cannot hold an unpaired
# surrogate), so this comparison would silently flip: CPython True, emitted
# False. Refused loudly in every lane.
def cmp_surrogate() -> bool:
    s = "\ud800"
    return s < "\ue000"
