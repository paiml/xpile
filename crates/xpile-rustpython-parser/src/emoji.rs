//! Emoji_Presentation code-point ranges (Unicode Emoji data), replacing the
//! unmaintained `unic-emoji-char` crate (RUSTSEC-2025-0098/0100).
//! Table copied from unic-emoji-char 0.9.0 (MIT OR Apache-2.0).

const EMOJI_PRESENTATION: &[(char, char)] = &[
    ('\u{231a}', '\u{231b}'),
    ('\u{23e9}', '\u{23ec}'),
    ('\u{23f0}', '\u{23f0}'),
    ('\u{23f3}', '\u{23f3}'),
    ('\u{25fd}', '\u{25fe}'),
    ('\u{2614}', '\u{2615}'),
    ('\u{2648}', '\u{2653}'),
    ('\u{267f}', '\u{267f}'),
    ('\u{2693}', '\u{2693}'),
    ('\u{26a1}', '\u{26a1}'),
    ('\u{26aa}', '\u{26ab}'),
    ('\u{26bd}', '\u{26be}'),
    ('\u{26c4}', '\u{26c5}'),
    ('\u{26ce}', '\u{26ce}'),
    ('\u{26d4}', '\u{26d4}'),
    ('\u{26ea}', '\u{26ea}'),
    ('\u{26f2}', '\u{26f3}'),
    ('\u{26f5}', '\u{26f5}'),
    ('\u{26fa}', '\u{26fa}'),
    ('\u{26fd}', '\u{26fd}'),
    ('\u{2705}', '\u{2705}'),
    ('\u{270a}', '\u{270b}'),
    ('\u{2728}', '\u{2728}'),
    ('\u{274c}', '\u{274c}'),
    ('\u{274e}', '\u{274e}'),
    ('\u{2753}', '\u{2755}'),
    ('\u{2757}', '\u{2757}'),
    ('\u{2795}', '\u{2797}'),
    ('\u{27b0}', '\u{27b0}'),
    ('\u{27bf}', '\u{27bf}'),
    ('\u{2b1b}', '\u{2b1c}'),
    ('\u{2b50}', '\u{2b50}'),
    ('\u{2b55}', '\u{2b55}'),
    ('\u{1f004}', '\u{1f004}'),
    ('\u{1f0cf}', '\u{1f0cf}'),
    ('\u{1f18e}', '\u{1f18e}'),
    ('\u{1f191}', '\u{1f19a}'),
    ('\u{1f1e6}', '\u{1f1ff}'),
    ('\u{1f201}', '\u{1f201}'),
    ('\u{1f21a}', '\u{1f21a}'),
    ('\u{1f22f}', '\u{1f22f}'),
    ('\u{1f232}', '\u{1f236}'),
    ('\u{1f238}', '\u{1f23a}'),
    ('\u{1f250}', '\u{1f251}'),
    ('\u{1f300}', '\u{1f320}'),
    ('\u{1f32d}', '\u{1f335}'),
    ('\u{1f337}', '\u{1f37c}'),
    ('\u{1f37e}', '\u{1f393}'),
    ('\u{1f3a0}', '\u{1f3ca}'),
    ('\u{1f3cf}', '\u{1f3d3}'),
    ('\u{1f3e0}', '\u{1f3f0}'),
    ('\u{1f3f4}', '\u{1f3f4}'),
    ('\u{1f3f8}', '\u{1f43e}'),
    ('\u{1f440}', '\u{1f440}'),
    ('\u{1f442}', '\u{1f4fc}'),
    ('\u{1f4ff}', '\u{1f53d}'),
    ('\u{1f54b}', '\u{1f54e}'),
    ('\u{1f550}', '\u{1f567}'),
    ('\u{1f57a}', '\u{1f57a}'),
    ('\u{1f595}', '\u{1f596}'),
    ('\u{1f5a4}', '\u{1f5a4}'),
    ('\u{1f5fb}', '\u{1f64f}'),
    ('\u{1f680}', '\u{1f6c5}'),
    ('\u{1f6cc}', '\u{1f6cc}'),
    ('\u{1f6d0}', '\u{1f6d2}'),
    ('\u{1f6eb}', '\u{1f6ec}'),
    ('\u{1f6f4}', '\u{1f6f8}'),
    ('\u{1f910}', '\u{1f93a}'),
    ('\u{1f93c}', '\u{1f93e}'),
    ('\u{1f940}', '\u{1f945}'),
    ('\u{1f947}', '\u{1f94c}'),
    ('\u{1f950}', '\u{1f96b}'),
    ('\u{1f980}', '\u{1f997}'),
    ('\u{1f9c0}', '\u{1f9c0}'),
    ('\u{1f9d0}', '\u{1f9e6}'),
];

/// True when `c` has the Emoji_Presentation property.
pub(crate) fn is_emoji_presentation(c: char) -> bool {
    EMOJI_PRESENTATION
        .binary_search_by(|&(lo, hi)| {
            if c < lo {
                std::cmp::Ordering::Greater
            } else if c > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_is_sorted_and_disjoint() {
        for w in EMOJI_PRESENTATION.windows(2) {
            assert!(w[0].0 <= w[0].1 && w[0].1 < w[1].0);
        }
    }
    #[test]
    fn classifies_known_points() {
        assert!(is_emoji_presentation('\u{1F600}'));
        assert!(is_emoji_presentation('\u{231A}'));
        assert!(!is_emoji_presentation('a'));
        assert!(!is_emoji_presentation('\u{00A9}'));
    }
}
