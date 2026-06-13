def url_kind(s: str) -> int:
    # startswith with a tuple of prefixes (true if any matches).
    if s.startswith(("http://", "https://")):
        return 1
    if s.startswith(("ftp://", "sftp://")):
        return 2
    return 0


def is_source(name: str) -> int:
    # endswith with a tuple of suffixes.
    if name.endswith((".py", ".rs", ".c")):
        return 1
    return 0


def single_prefix(s: str) -> int:
    # 1-arg form still works.
    if s.startswith("#"):
        return 1
    return 0
