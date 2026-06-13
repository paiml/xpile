# PMAT-502cs (Tranche 2): str.zfill(width) — left-pad with zeros to width,
# sign-aware (a leading -/+ stays first; zeros go after it). Already-wide
# strings are returned unchanged.
def pad(s: str) -> str:
    return s.zfill(5)
