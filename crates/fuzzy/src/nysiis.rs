//! NYSIIS — direct port of `nysiis()` in `src/fuzzy.pyx`.
//!
//! The Cython source is pure Python semantics (the `cdef char *` locals in the
//! original are never used as C strings in a way that changes the result), so
//! this is a straight transliteration of the Python control flow.
//!
//! Two upstream warts are preserved deliberately — see `DECISIONS.md` 06/07:
//!   * `AY` sits in the unconditional transform table rather than the
//!     not-first table, so `FLOYD` codes as `FLYD`, not the textbook `FLAD`
//!     (upstream issue #8).
//!   * `MAC` -> `MC` shortens the string but the suffix window is only
//!     compensated by one, which is what makes `MACINTOSH` and `MCINTOSH`
//!     agree; textbook NYSIIS recomputes the window.

/// Applied at any position.
fn transform(x: &[u8]) -> Option<&'static str> {
    Some(match x {
        b"AY" => "Y",
        b"DG" => "G",
        b"E" => "A",
        b"EY" => "Y",
        b"GHT" => "GT",
        b"K" => "C",
        b"KN" => "N",
        b"I" => "A",
        b"IY" => "Y",
        b"O" => "A",
        b"OY" => "Y",
        b"PH" => "F",
        b"SH" => "S",
        b"SCH" => "S",
        b"U" => "A",
        b"UY" => "Y",
        b"WR" => "R",
        b"YW" => "Y",
        _ => return None,
    })
}

/// Applied only when not at the first position.
fn transform_not_first(x: &[u8]) -> Option<&'static str> {
    Some(match x {
        b"AH" => "A",
        b"AW" => "A",
        b"EH" => "A",
        b"EV" => "AF",
        b"EW" => "A",
        b"HA" => "A",
        b"HE" => "A",
        b"HI" => "A",
        b"HO" => "A",
        b"HU" => "A",
        b"IH" => "A",
        b"IW" => "A",
        b"M" => "N",
        b"OH" => "A",
        b"OW" => "A",
        b"Q" => "G",
        b"UH" => "A",
        b"UW" => "A",
        b"Z" => "S",
        _ => return None,
    })
}

/// Applied only strictly inside the word.
fn transform_middle(x: &[u8]) -> Option<&'static str> {
    match x {
        b"Y" => Some("A"),
        _ => None,
    }
}

fn suffix_map(x: &[u8]) -> Option<&'static str> {
    Some(match x {
        b"IX" => "IC",
        b"EX" => "EC",
        b"YE" => "Y",
        b"EE" => "Y",
        b"IE" => "Y",
        b"DT" => "D",
        b"RT" => "D",
        b"RD" => "D",
        b"NT" => "D",
        b"ND" => "D",
        _ => return None,
    })
}

/// NYSIIS code for `input`.
///
/// Matches `fuzzy.nysiis` on every input the Python function accepts. Unlike
/// `Soundex`/`DMetaphone` this never touched a C string upstream, so there is
/// no ASCII restriction: non-A-Z characters are stripped, exactly as the
/// original's `re.sub('[^A-Z]', '', s.upper())` does.
pub fn nysiis(input: &str) -> String {
    // Strip out anything non-alpha. `str::to_uppercase` is Unicode-aware, like
    // Python's `str.upper()`, so 'ß' still expands to "SS" before filtering.
    let s: Vec<u8> = input
        .to_uppercase()
        .bytes()
        .filter(|c| c.is_ascii_uppercase())
        .collect();

    let mut start: usize = 0;
    let mut stop: usize = s.len();

    // Python's `first = ''` then `'' in 'AEIOU'` is True, but an empty `r`
    // makes the result empty anyway — so bail out here instead.
    let Some(&first) = s.first() else {
        return String::new();
    };

    // Find index without trailing S/Z.
    let mut i = stop;
    while i > 0 && (s[i - 1] == b'S' || s[i - 1] == b'Z') {
        i -= 1;
    }
    stop = i;

    // Initial MAC -> MC, PF -> F.
    let mut s = s;
    if s.starts_with(b"MAC") {
        s.remove(1); // "MAC..." -> "MC..."
        stop = stop.saturating_sub(1);
    } else if s.starts_with(b"PF") {
        start = 1;
    }

    // Translate 2-character suffix elements.
    let mut suffix = String::new();
    while stop.saturating_sub(start) > 2 {
        match suffix_map(&s[stop - 2..stop]) {
            Some(mapped) => {
                suffix = format!("{mapped}{suffix}");
                stop -= 2;
            }
            None => break,
        }
    }

    // Python slices clamp; Rust ranges panic. The arithmetic above cannot
    // produce an out-of-range window, but this is a fuzz target — no panics.
    let stop = stop.min(s.len()).max(start);
    let mut s: Vec<u8> = s[start..stop].to_vec();
    s.extend_from_slice(suffix.as_bytes());

    // Build a list of adjacent components while performing transformations.
    // NOTE: `start` is reset to 0 here in the original, so the "not first"
    // tables key off absolute position 0, not the PF-adjusted start.
    let mut r: Vec<u8> = Vec::with_capacity(s.len());
    let mut i: usize = 0;
    let start: usize = 0;
    let stop: usize = s.len();
    while i < stop {
        let remain = stop - i; // number of letters including this one

        let mut app: Option<&'static str> = None;
        let mut used = 1usize;

        for l in [3usize, 2, 1] {
            if remain >= l {
                let x = &s[i..i + l];
                if let Some(v) = transform(x) {
                    app = Some(v);
                    used = l;
                    break;
                } else if i > start {
                    if let Some(v) = transform_not_first(x) {
                        app = Some(v);
                        used = l;
                        break;
                    } else if i < stop - 1 {
                        if let Some(v) = transform_middle(x) {
                            app = Some(v);
                            used = l;
                            break;
                        }
                    }
                }
            }
        }

        match app {
            Some(v) => {
                r.extend_from_slice(v.as_bytes());
                i += used;
            }
            None => {
                r.push(s[i]);
                i += 1;
            }
        }
    }

    // Remove trailing vowels.
    let mut stop = r.len();
    while stop > 0 && matches!(r[stop - 1], b'A' | b'E' | b'I' | b'O' | b'U') {
        stop -= 1;
    }

    // If first char of original string is a vowel, use it.
    if matches!(first, b'A' | b'E' | b'I' | b'O' | b'U') {
        if r.is_empty() {
            r.push(first);
        } else {
            r[0] = first;
        }
    }

    // Filter out repeated characters.
    let mut q: Vec<u8> = Vec::with_capacity(stop);
    let mut last: Option<u8> = None;
    for &x in &r[..stop.min(r.len())] {
        if Some(x) == last {
            continue;
        }
        q.push(x);
        last = Some(x);
    }

    String::from_utf8(q).expect("A-Z only by construction")
}
