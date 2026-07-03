# PMAT-1092 (skeptic pass PMAT-1090, A-p13): an `except ... as e` name read
# AFTER its handler with no prior binding. CPython deletes the `as` name at
# handler exit (the read raises UnboundLocalError); the old behavior was a
# MISLEADING refusal ("declared return type Str but body produces I64" — type
# inference falling over the unbound name). Now refused with the deletion
# truth + the copy-inside-the-handler workaround.
def as_no_prior() -> str:
    try:
        raise ValueError("boom")
    except ValueError as e:
        pass
    return e
