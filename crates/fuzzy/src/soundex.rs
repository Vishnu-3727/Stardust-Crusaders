//! Soundex — port of `cdef class Soundex` in `src/fuzzy.pyx`.
//!
//! This is the one function that is **not** bug-compatible with upstream, and
//! it cannot be: the original is undefined behavior. `cs = s` assigns a `char *`
//! from a temporary ASCII encoding of the Python string; that temporary's
//! refcount hits zero at the end of the statement, so `strlen(cs)` and every
//! read after it touch freed memory. The observable result is nondeterministic —
//! usually `''` (upstream issues #14, #17, #20), sometimes garbage, and the
//! surrounding allocator state is what decides. There is no faithful port of a
//! read-after-free.
//!
//! So this implements what the code *computes when the pointer happens to still
//! be valid* — the coding rules exactly as written, including their deviations
//! from textbook Soundex:
//!
//!   * The first coded digit is always emitted even when it repeats the code of
//!     the initial letter (`written == 1 ||` in the dedup test), so `PFISTER`
//!     codes `P123`, not the textbook `P236`.
//!   * The result is always padded to exactly `size` characters, and `size` is
//!     a hard cap rather than the conventional 4.
//!   * H and W do not separate duplicate consonants — dedup is on adjacent
//!     *output* digits only.
//!
//! See `DECISIONS.md` 02 for the divergence write-up.

/// A-Z code table, indexed by `letter - b'A'`. Verbatim from the original.
const MAP: &[u8; 26] = b"01230120022455012623010202";

/// Soundex code of `input`, padded to exactly `size` characters.
///
/// `input` is bytes because the original walked a C string: the scan stops at
/// the first NUL and non-ASCII bytes are simply skipped (they are neither
/// `a-z` nor `A-Z`). The Python layer never gets that far — it raises
/// `UnicodeEncodeError` while encoding, which [`crate::soundex`] reproduces.
pub fn soundex_bytes(input: &[u8], size: usize) -> String {
    // `size == 0` is the one input where the original overruns its own buffer:
    // it allocates `size + 1` bytes, and the `written == size` break can never
    // fire because `written` is already 1 by the time it is first tested. The
    // C then writes the terminator to `out[0]` and returns "". Result matches;
    // the heap smash does not, and will not.
    if size == 0 {
        return String::new();
    }

    // strlen(): the C string ends at the first NUL.
    let bytes = match input.iter().position(|&b| b == 0) {
        Some(n) => &input[..n],
        None => input,
    };

    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut written = 0usize;

    for &b in bytes {
        let c = b.to_ascii_uppercase();
        if c.is_ascii_uppercase() {
            if written == 0 {
                out.push(c);
                written = 1;
            } else {
                let digit = MAP[(c - b'A') as usize];
                if digit != b'0' && (written == 1 || out[written - 1] != digit) {
                    out.push(digit);
                    written += 1;
                }
            }
        }
        // The C tests this after every input character, not only after a write,
        // so `size == 0` breaks out before anything is coded.
        if written == size {
            break;
        }
    }

    while out.len() < size {
        out.push(b'0');
    }

    String::from_utf8(out).expect("A-Z and digits only by construction")
}
