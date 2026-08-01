//! Rust port of [yougov/fuzzy](https://github.com/yougov/fuzzy) — Soundex,
//! NYSIIS and Double Metaphone.
//!
//! The goal is **behavioral equivalence with the shipped library**, not with
//! the published algorithms. Where the original disagrees with the textbook, so
//! does this crate, and the disagreement is documented at the call site and in
//! `DECISIONS.md`. The one exception is `Soundex`, whose upstream behavior is
//! undefined (read-after-free) and therefore unportable; see [`soundex`].
//!
//! Two layers:
//!   * [`nysiis`], [`soundex_bytes`], [`dmetaphone_raw`] — the algorithms.
//!   * [`soundex`], [`dmetaphone`] — the same functions wearing the Python
//!     API's clothes: ASCII-only input, `None` for absent codes, `size` caps.

#![forbid(unsafe_code)]

mod dmetaphone;
mod nysiis;
mod soundex;

pub use dmetaphone::dmetaphone_raw;
pub use nysiis::nysiis;
pub use soundex::soundex_bytes;

use std::fmt;

/// The Python API is a Cython extension declared
/// `# cython: c_string_type=unicode, c_string_encoding=ascii`, so passing a
/// non-ASCII string raises `UnicodeEncodeError` before any phonetic code runs.
/// Upstream issue #15 is exactly this. NYSIIS is unaffected — it never crosses
/// into C.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsciiError {
    /// Byte offset of the offending character in the input.
    pub position: usize,
    pub ch: char,
}

impl fmt::Display for AsciiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "'ascii' codec can't encode character '\\u{:04x}' in position {}: ordinal not in range(128)",
            self.ch as u32, self.position
        )
    }
}

impl std::error::Error for AsciiError {}

fn as_ascii(s: &str) -> Result<&[u8], AsciiError> {
    match s.char_indices().find(|(_, c)| !c.is_ascii()) {
        Some((position, ch)) => Err(AsciiError { position, ch }),
        None => Ok(s.as_bytes()),
    }
}

/// `fuzzy.Soundex(size)(s)`.
///
/// **Diverges from upstream on purpose.** The original returns `''` or garbage
/// for most inputs because it reads a freed buffer (issues #14/#17/#20); this
/// returns what the coding rules actually compute. Everything else about the
/// original's shape is kept: the `size` cap, the zero padding, and the
/// non-textbook dedup rule. Consequence: the upstream test
/// `test_soundex_result` — marked `xfail(reason="issue #14")` — passes here.
pub fn soundex(s: &str, size: usize) -> Result<String, AsciiError> {
    Ok(soundex_bytes(as_ascii(s)?, size))
}

/// `fuzzy.DMetaphone(size)(s)`.
///
/// Returns `(primary, secondary)` with the Python layer's `None` collapsing:
/// an empty code becomes `None`, and the secondary becomes `None` when it is
/// identical to the primary. `size` of 0 means "unbounded" — the constructor
/// is `self.size = size or 99999` — and in practice the C has already cut both
/// codes to four characters.
pub fn dmetaphone(s: &str, size: usize) -> Result<(Option<String>, Option<String>), AsciiError> {
    let size = if size == 0 { 99_999 } else { size };
    let (primary, secondary) = dmetaphone_raw(as_ascii(s)?);

    // `if o1 == o2: o2 = None` happens before slicing, in the original.
    let secondary = if secondary == primary { String::new() } else { secondary };

    fn cap(code: String, size: usize) -> Option<String> {
        // `o and o[:size] or None`: empty in, None out.
        if code.is_empty() {
            None
        } else {
            let mut code = code;
            code.truncate(size);
            Some(code)
        }
    }

    Ok((cap(primary, size), cap(secondary, size)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectors from the upstream README and test suite.
    #[test]
    fn readme_vectors() {
        assert_eq!(soundex("fuzzy", 4).unwrap(), "F200");
        assert_eq!(nysiis("fuzzy"), "FASY");
        assert_eq!(
            dmetaphone("fuzzy", 0).unwrap(),
            (Some("FS".to_string()), None)
        );
    }

    /// The upstream test file, transcribed. `test_soundex_result` is marked
    /// xfail upstream (issue #14) and passes here — that is the headline.
    #[test]
    fn upstream_suite() {
        assert_eq!(dmetaphone("mayer", 0).unwrap(), (Some("MR".to_string()), None));
        assert_eq!(soundex("FancyFree", 4).unwrap(), "F521");
        // Still fails upstream's expectation, but for a reason that is the
        // test's, not the port's: Soundex pads to `size`, so 8 means 8.
        assert_eq!(soundex("Test", 8).unwrap(), "T2300000");
        // Issue #15: non-ASCII raises before reaching the algorithm.
        assert!(soundex("Jéroboam", 8).is_err());
    }

    /// The two brace bugs. If a future refactor "fixes" the algorithm to match
    /// the textbook, these fail — which is the point.
    #[test]
    fn upstream_brace_bugs_are_preserved() {
        // Soft C is coded K and swallows the next letter (DECISIONS.md 04).
        assert_eq!(dmetaphone("cent", 0).unwrap().0.unwrap(), "KNT"); // not SNT
        assert_eq!(dmetaphone("city", 0).unwrap().0.unwrap(), "KT"); // not ST
        // SC not followed by H falls through to generic S (DECISIONS.md 05).
        assert_eq!(dmetaphone("science", 0).unwrap().0.unwrap(), "SKNK"); // not SNS
        // ...but SCH still works, because that branch is live.
        assert_eq!(dmetaphone("school", 0).unwrap().0.unwrap(), "SKL");
    }

    #[test]
    fn nysiis_upstream_warts() {
        assert_eq!(nysiis("FLOYD"), "FLYD"); // issue #8: textbook says FLAD
        assert_eq!(nysiis("MACINTOSH"), nysiis("MCINTOSH"));
    }

    #[test]
    fn edge_cases_do_not_panic() {
        for s in ["", " ", "\t", "1234", "X", "S", "MAC", "PF", "SZ", &"A".repeat(300)] {
            let _ = soundex(s, 4).unwrap();
            let _ = nysiis(s);
            let _ = dmetaphone(s, 0).unwrap();
        }
        assert_eq!(soundex("", 4).unwrap(), "0000");
        assert_eq!(soundex("anything", 0).unwrap(), "");
        assert_eq!(nysiis(""), "");
        assert_eq!(dmetaphone("", 0).unwrap(), (None, None));
    }
}
