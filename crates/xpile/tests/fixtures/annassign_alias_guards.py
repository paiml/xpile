# PMAT-1019 (sweep #9): ANNOTATION-BLINDNESS — every alias/launder/shared-row
# guard lived only in lower_assign; `b: list[int] = a` bypassed the whole
# suite (annotated read-only aliases were E0382; annotated [row,row] / [...]*n
# silently cloned; annotated launder bindings silently detached — ~10
# confirmed findings, one root cause). The suite is now a shared helper
# (apply_alias_dispositions) called by BOTH binding forms.
def annotated_read_only_alias() -> int:
    a: list[int] = [1, 2, 3]
    b: list[int] = a
    return len(b) + len(a) + b[0]
