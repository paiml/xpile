# PMAT-622: sorting a list whose element embeds a float (a tuple with a float, or
# a nested list of floats) emitted Vec::sort(), which needs Ord — but f64 is not
# Ord → E0277. The keyless float-sort detection now recurses through tuples and
# nested lists, routing to the partial_cmp path. An int-only tuple still uses
# .sort() (it is Ord).
def srt_tuple(xs: list[tuple[float, int]]) -> list[tuple[float, int]]:
    return sorted(xs)


def srt_nested(xs: list[list[float]]) -> list[list[float]]:
    return sorted(xs)


def inplace_tuple(xs: list[tuple[float, int]]) -> list[tuple[float, int]]:
    xs.sort()
    return xs


def srt_int_tuple(xs: list[tuple[int, int]]) -> list[tuple[int, int]]:
    return sorted(xs)
