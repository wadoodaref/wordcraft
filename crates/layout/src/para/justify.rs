use super::model::ClKind;
use super::proof::{ProofRun, lang_at};
use super::{ParaLayout, SOFT_HYPHEN};
use wordcraft_doc::props::JustifyStyle;

/// Word's default hyphenation zone (0.25"), points.
pub(super) const HYPHENATION_ZONE: f32 = wordcraft_doc::DEFAULT_HYPHENATION_ZONE;

/// How much of their width the spaces of a justified line may give up to fit more text (Word
/// 2013+, compatibility mode 15).
const MAX_SPACE_SHRINK: f32 = 0.25;

/// How far a word (with the space before it) may overrun a justified line and still be fitted
/// onto it by shrinking the line's spaces, as a share of its own width.
const MAX_WORD_OVERRUN: f32 = 0.355;

/// How far a justified line's text may run past its right edge, its spaces (`spaces` wide in
/// all) shrinking to make room, when the word now ending the line is `word` wide with the space
/// before it. Measured in Word for Mac (compatibility mode 15, synthetic justified paragraphs):
/// at most a quarter of the spaces' width, and at most 0.355 of the word's own (0.351 to 0.360
/// across words of 2.7 to 80 pt and spaces of 3.3 and 6.7 pt).
pub(super) fn space_room(spaces: f32, word: f32) -> f32 {
    (spaces * MAX_SPACE_SHRINK).min(word.max(0.0) * MAX_WORD_OVERRUN)
}

/// Take `-slack` (an overflow) out of the line's inner spaces, in proportion to their widths and
/// never more than [`MAX_SPACE_SHRINK`] of them. Trailing spaces hang and are left alone.
pub(super) fn shrink_line_spaces(pl: &ParaLayout, xs: &mut [f32], c0: usize, c1: usize, content_end: f32, slack: f32) {
    let spaces: Vec<(usize, f32)> = (c0..c1)
        .filter_map(|k| {
            let c = pl.clusters.get(k).filter(|c| c.kind == ClKind::Space)?;
            (xs.get(k - c0).copied().unwrap_or(f32::MAX) < content_end - 0.01).then_some((k, c.adv))
        })
        .collect();
    let total: f32 = spaces.iter().map(|s| s.1).sum();
    if total <= 0.0 || !slack.is_finite() {
        return;
    }
    let ratio = (-slack / total).clamp(0.0, MAX_SPACE_SHRINK);
    let mut take = 0.0;
    let mut next = spaces.iter().peekable();
    for k in c0..c1 {
        if let Some(v) = xs.get_mut(k - c0) {
            *v -= take;
        }
        if let Some((_, adv)) = next.next_if(|s| s.0 == k) {
            take += adv * ratio;
        }
    }
    if let Some(v) = xs.last_mut() {
        *v -= take;
    }
}

/// The hyphen glyph and advance in a style (cached per paragraph).
pub(super) fn hyphen_glyph(pl: &ParaLayout, style: u32, cache: &mut Vec<(u32, u32, f32)>) -> (u32, u32, f32) {
    if let Some(h) = cache.iter().find(|h| h.0 == style) {
        return *h;
    }
    let h = pl
        .styles
        .get(style as usize)
        .and_then(|st| {
            let g = wordcraft_fonts::shape(&st.face, "-", &[], |c| c).into_iter().next()?;
            let k = st.size / st.face.upem.max(1.0) as f32 * st.rc.scale / 100.0;
            Some((style, g.gid, g.x_advance as f32 * k))
        })
        .unwrap_or((style, 0, 0.0));
    cache.push(h);
    h
}

/// Clusters after which a line may end with a hyphen: soft hyphens always, and dictionary or
/// pattern hyphenation points of each word, in the language of its run ([`ProofRun`]), when
/// automatic hyphenation is on (words in capitals only with `caps`). Both are found in `text`, the paragraph as laid out (without what
/// the layout leaves out, [`left_out`]), and `to_para` maps the end of a char there to the
/// paragraph's text.
pub(super) fn hyphenation_points(
    text: &str,
    to_para: impl Fn(usize) -> Option<usize>,
    pl: &ParaLayout,
    auto: bool,
    caps: bool,
    runs: &[ProofRun],
) -> Vec<u32> {
    let mut bytes: Vec<usize> = text.char_indices().filter(|(_, c)| *c == SOFT_HYPHEN).map(|(i, c)| i + c.len_utf8()).collect();
    // Vietnamese words are single syllables and never take a hyphen: no automatic hyphenation in
    // a Vietnamese paragraph, nor in a word written with a Vietnamese-only letter (`Nguyễn`).
    if auto && !wordcraft_proof::vi::looks_vietnamese(text) {
        let lim = wordcraft_proof::hyphen::Limits::default();
        let mut start: Option<usize> = None;
        for (i, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
            let word_char = c.is_alphabetic() || c == '\'' || c == '\u{2019}';
            match (start, word_char) {
                (None, true) => start = Some(i),
                (Some(a), false) => {
                    // The patterns are for Latin-script languages: never hyphenate Persian/Arabic words.
                    // Words in capitals (`NASA`, `README`) only when the document allows it.
                    let all_caps = |w: &str| w.chars().filter(|c| c.is_alphabetic()).all(char::is_uppercase);
                    if let Some(w) = text.get(a..i)
                        && w.chars().count() >= lim.min_word
                        && !w.chars().any(wordcraft_fonts::is_rtl)
                        && !wordcraft_proof::vi::is_vietnamese_word(w)
                        && (caps || !all_caps(w))
                    {
                        // The word's language: its first letter's run, found by its place in `p.text`.
                        let first = w.chars().next().map_or(1, char::len_utf8);
                        let lang = to_para(a + first).map(|e| lang_at(runs, e.saturating_sub(first))).unwrap_or_default();
                        let offs: Vec<usize> = w.char_indices().map(|(o, _)| o).collect();
                        for pt in wordcraft_proof::hyphen::hyphen_points_in(w, &lim, lang) {
                            if let Some(o) = offs.get(pt) {
                                bytes.push(a + o);
                            }
                        }
                    }
                    start = None;
                }
                _ => {}
            }
        }
    }
    let mut bytes: Vec<usize> = bytes.into_iter().filter_map(to_para).collect();
    if bytes.is_empty() {
        return Vec::new();
    }
    bytes.sort_unstable();
    let mut out: Vec<u32> = pl
        .clusters
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c.kind, ClKind::Text | ClKind::Marker) && bytes.binary_search(&c.end).is_ok())
        .map(|(i, _)| i as u32)
        .collect();
    out.dedup();
    out
}

/// Whether a Unicode character is an Arabic dual-joining letter.
pub fn is_arabic_dual_joining(c: char) -> bool {
    matches!(
        c,
        '\u{0628}' // Beh
        | '\u{062A}' // Teh
        | '\u{062B}' // Theh
        | '\u{062C}' // Jeem
        | '\u{062D}' // Hah
        | '\u{062E}' // Khah
        | '\u{0633}' // Seen
        | '\u{0634}' // Sheen
        | '\u{0635}' // Sad
        | '\u{0636}' // Dad
        | '\u{0637}' // Tah
        | '\u{0638}' // Zah
        | '\u{0639}' // Ain
        | '\u{063A}' // Ghain
        | '\u{0641}' // Feh
        | '\u{0642}' // Qaf
        | '\u{0643}' // Kaf
        | '\u{0644}' // Lam
        | '\u{0645}' // Meem
        | '\u{0646}' // Noon
        | '\u{0647}' // Heh
        | '\u{064A}' // Yeh
        | '\u{0626}' // Yeh with Hamza above
        | '\u{0640}' // Tatweel
        | '\u{067E}' // Peh
        | '\u{0686}' // Tcheh
        | '\u{06AF}' // Gaf
        | '\u{06A9}' // Keheh
        | '\u{06CC}' // Farsi Yeh
        | '\u{0679}' // Tteh
        | '\u{06BA}' // Noon Ghunna
        | '\u{06BE}' // Heh Doachashmee
    )
}

/// Whether a Unicode character is an Arabic right-joining letter (connects only to preceding).
pub fn is_arabic_right_joining(c: char) -> bool {
    matches!(
        c,
        '\u{0622}' // Alef with Madda
        | '\u{0623}' // Alef with Hamza above
        | '\u{0624}' // Waw with Hamza above
        | '\u{0625}' // Alef with Hamza below
        | '\u{0627}' // Alef
        | '\u{062F}' // Dal
        | '\u{0630}' // Thal
        | '\u{0631}' // Reh
        | '\u{0632}' // Zain
        | '\u{0648}' // Waw
        | '\u{0629}' // Teh Marbuta
        | '\u{0649}' // Alef Maksura
        | '\u{0671}' // Alef Wasla
        | '\u{0688}' // Ddal
        | '\u{0691}' // Rreh
        | '\u{0698}' // Jeh
    )
}

/// Extract base character by stripping Arabic diacritical marks (harakat).
pub fn arabic_base_char(s: &str) -> Option<char> {
    s.chars().find(|c| !matches!(u32::from(*c), 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06ED | 0xFE70..=0xFE7F))
}

#[derive(Clone, Copy, Debug)]
pub struct KashidaCandidate {
    pub cluster: usize,
    pub priority: u8,
    pub word: usize,
}

/// Find all valid kashida connection points on the line.
pub fn find_kashida_candidates(
    pl: &ParaLayout,
    text: &str,
    (c0, c1, text_from): (usize, usize, usize),
    xs: &[f32],
    content_end: f32,
    style: JustifyStyle,
) -> Vec<KashidaCandidate> {
    if c1 <= text_from + 1 {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    let mut word = 0;
    for k in text_from..c1.saturating_sub(1) {
        let Some(c_curr) = pl.clusters.get(k) else { continue };
        let Some(c_next) = pl.clusters.get(k + 1) else { continue };

        if c_curr.kind == ClKind::Space {
            word += 1;
            continue;
        }

        if c_curr.kind != ClKind::Text || c_next.kind != ClKind::Text {
            continue;
        }

        if c_curr.end != c_next.start {
            continue;
        }

        if xs.get(k - c0).copied().unwrap_or(f32::MAX) >= content_end - 0.01 {
            continue;
        }

        let s1 = text.get(c_curr.start..c_curr.end).unwrap_or("");
        let s2 = text.get(c_next.start..c_next.end).unwrap_or("");

        let Some(b1) = arabic_base_char(s1) else { continue };
        let Some(b2) = arabic_base_char(s2) else { continue };

        if is_arabic_dual_joining(b1) && (is_arabic_dual_joining(b2) || is_arabic_right_joining(b2)) {
            let priority = if matches!(b1, 'س' | 'ش' | 'ص' | 'ض') {
                3
            } else if k + 2 >= c1 || pl.clusters.get(k + 2).is_some_and(|c| c.kind != ClKind::Text) {
                2
            } else if k + 3 >= c1 || pl.clusters.get(k + 3).is_some_and(|c| c.kind != ClKind::Text) {
                let s3 = pl.clusters.get(k + 2).and_then(|c| text.get(c.start..c.end)).unwrap_or("");
                if arabic_base_char(s3).is_some_and(|c| matches!(c, 'ة' | 'ه' | 'ا' | 'ي' | 'ك')) { 2 } else { 1 }
            } else {
                1
            };

            if style == JustifyStyle::LowKashida && priority < 3 {
                continue;
            }

            candidates.push(KashidaCandidate { cluster: k, priority, word });
        }
    }

    let mut by_word: Vec<KashidaCandidate> = Vec::new();
    for cand in candidates {
        if let Some(existing) = by_word.iter_mut().find(|c| c.word == cand.word) {
            if cand.priority > existing.priority {
                *existing = cand;
            }
        } else {
            by_word.push(cand);
        }
    }
    candidates = by_word;

    candidates
}

/// Pre-allocate kashida reserves on Arabic words before line breaking for Medium and High justification.
pub fn compute_kashida_reserves(pl: &ParaLayout, text: &str) -> Vec<(usize, f32)> {
    let Some(style) = pl.rp.justify_style else { return Vec::new() };

    let reserve = match style {
        JustifyStyle::MediumKashida => 3.2,
        JustifyStyle::HighKashida => 6.8,
        _ => return Vec::new(),
    };

    let n = pl.clusters.len();
    if n < 2 {
        return Vec::new();
    }

    let mut reserves = Vec::new();
    let mut word = 0;
    let mut word_candidates: Vec<KashidaCandidate> = Vec::new();

    let flush_word = |word_cands: &mut Vec<KashidaCandidate>, res: &mut Vec<(usize, f32)>| {
        if word_cands.is_empty() {
            return;
        }
        word_cands.sort_by_key(|c| std::cmp::Reverse(c.priority));
        if let Some(best) = word_cands.first() {
            res.push((best.cluster, reserve));
        }
        word_cands.clear();
    };

    for k in 0..n - 1 {
        let Some(c_curr) = pl.clusters.get(k) else { continue };
        let Some(c_next) = pl.clusters.get(k + 1) else { continue };

        if c_curr.kind == ClKind::Space {
            flush_word(&mut word_candidates, &mut reserves);
            word += 1;
            continue;
        }

        if c_curr.kind != ClKind::Text || c_next.kind != ClKind::Text || c_curr.end != c_next.start {
            continue;
        }

        let s1 = text.get(c_curr.start..c_curr.end).unwrap_or("");
        let s2 = text.get(c_next.start..c_next.end).unwrap_or("");

        let Some(b1) = arabic_base_char(s1) else { continue };
        let Some(b2) = arabic_base_char(s2) else { continue };

        if is_arabic_dual_joining(b1) && (is_arabic_dual_joining(b2) || is_arabic_right_joining(b2)) {
            let priority = if matches!(b1, 'س' | 'ش' | 'ص' | 'ض') {
                3
            } else if k + 2 >= n || pl.clusters.get(k + 2).is_some_and(|c| c.kind != ClKind::Text) {
                2
            } else {
                1
            };
            word_candidates.push(KashidaCandidate { cluster: k, priority, word });
        }
    }
    flush_word(&mut word_candidates, &mut reserves);

    reserves
}

/// Apply kashida elongation and distribute remaining slack to spaces.
pub fn apply_kashida_justification(
    pl: &ParaLayout,
    xs: &mut [f32],
    (c0, c1, text_from): (usize, usize, usize),
    spaces: &[usize],
    candidates: Vec<KashidaCandidate>,
    slack: f32,
    style: JustifyStyle,
) -> Vec<(usize, f32)> {
    if candidates.is_empty() || slack <= 0.0 {
        return Vec::new();
    }

    if style == JustifyStyle::LowKashida && slack < 0.5 {
        return Vec::new();
    }

    let (kashida_slack, max_per_cand) = match style {
        JustifyStyle::LowKashida => (slack, 10.0),
        JustifyStyle::MediumKashida => {
            let budget = (candidates.len() as f32 * 14.0).min(slack);
            (budget, 14.0)
        }
        JustifyStyle::HighKashida => (slack, 30.0),
        JustifyStyle::Thai => return Vec::new(),
    };

    let total_weight: f32 = candidates.iter().map(|c| c.priority as f32).sum();
    let mut selected_kashidas: Vec<(usize, f32, f32)> = Vec::with_capacity(candidates.len());

    if total_weight > 0.0 {
        for cand in &candidates {
            let initial = pl.kashida_reserves.iter().find(|(ci, _)| *ci == cand.cluster).map(|(_, r)| *r).unwrap_or(0.0);
            let share = (kashida_slack * (cand.priority as f32 / total_weight)).min(max_per_cand);
            let total_adv = initial + share;
            if total_adv > 0.01 {
                selected_kashidas.push((cand.cluster, total_adv, share));
            }
        }
    }

    selected_kashidas.sort_by_key(|(k, _, _)| *k);

    let kashida_used: f32 = selected_kashidas.iter().map(|(_, _, s)| *s).sum();
    let remaining_slack = (slack - kashida_used).max(0.0);
    let per_space = if !spaces.is_empty() { remaining_slack / spaces.len() as f32 } else { 0.0 };

    let mut add = 0.0;
    let mut si = 0;
    let mut ki = 0;
    for k in text_from..c1 {
        if let Some(v) = xs.get_mut(k - c0) {
            *v += add;
        }
        if spaces.get(si) == Some(&k) {
            add += per_space;
            si += 1;
        }
        if let Some(&(ci, _, share)) = selected_kashidas.get(ki)
            && ci == k
        {
            add += share;
            ki += 1;
        }
    }
    if let Some(v) = xs.last_mut() {
        *v += add;
    }

    selected_kashidas.into_iter().map(|(k, total, _)| (k, total)).collect()
}
