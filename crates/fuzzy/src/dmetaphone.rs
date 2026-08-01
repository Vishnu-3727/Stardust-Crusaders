//! Double Metaphone — direct port of `src/double_metaphone.c` from yougov/fuzzy
//! (Maurice Aubrey's 1999 C implementation of Lawrence Philips' algorithm).
//!
//! This is a *transliteration*, not a reimplementation. Branch order, window
//! predicates and cursor arithmetic follow the C line for line, including two
//! brace bugs in the original that turn real branches into dead code (see
//! `DECISIONS.md`, entries 04 and 05). Reproducing them is the point: behavioral
//! equivalence with the shipped library beats agreement with the published
//! algorithm.

/// The uppercased input buffer, padded like the C does so the window predicates
/// can index past the end of the word without bounds-checking every call site.
///
/// The C pads `original` with five spaces via `MetaphAdd(original, "     ")`,
/// which also bumps `original->length`. So `GetAt`/`IsVowel`/`StringAt` are
/// bounded by the *padded* length, while the main loop is bounded by the real
/// `length` captured before padding.
struct Word {
    buf: Vec<u8>,
    /// Padded length — the bound used by `get_at`, `is_vowel`, `string_at`.
    padded_len: i32,
}

const PAD: &[u8] = b"     ";

impl Word {
    fn new(s: &[u8]) -> Word {
        let mut buf: Vec<u8> = s.iter().map(|c| c.to_ascii_uppercase()).collect();
        buf.extend_from_slice(PAD);
        let padded_len = buf.len() as i32;
        Word { buf, padded_len }
    }

    /// `GetAt`: out-of-range reads yield NUL, exactly as the C returns `'\0'`.
    fn get_at(&self, pos: i32) -> u8 {
        if pos < 0 || pos >= self.padded_len {
            return 0;
        }
        self.buf[pos as usize]
    }

    fn is_vowel(&self, pos: i32) -> bool {
        if pos < 0 || pos >= self.padded_len {
            return false;
        }
        matches!(self.buf[pos as usize], b'A' | b'E' | b'I' | b'O' | b'U' | b'Y')
    }

    /// `StringAt`: does `start` begin one of `tests`, comparing `n` bytes?
    ///
    /// The C uses `strncmp` against a NUL-terminated buffer, so a comparison
    /// that runs off the end fails on the NUL rather than reading garbage —
    /// `get_at` returning 0 past the end reproduces that byte for byte.
    fn string_at(&self, start: i32, n: usize, tests: &[&str]) -> bool {
        if start < 0 || start >= self.padded_len {
            return false;
        }
        for test in tests {
            let t = test.as_bytes();
            debug_assert!(t.len() >= n, "StringAt test {test:?} shorter than length {n}");
            if (0..n).all(|i| self.get_at(start + i as i32) == t[i]) {
                return true;
            }
        }
        false
    }

    /// `SlavoGermanic`: substring search over the whole padded buffer.
    fn slavo_germanic(&self) -> bool {
        // "WITZ" is redundant — any "WITZ" already contains "W" — but it is in
        // the original, so it stays in the comment rather than the code.
        self.buf
            .windows(2)
            .any(|w| w == b"CZ")
            || self.buf.contains(&b'W')
            || self.buf.contains(&b'K')
    }
}

/// Accumulator for a metaphone code. `MetaphAdd` in the C.
#[derive(Default)]
struct Code(Vec<u8>);

impl Code {
    fn add(&mut self, s: &str) {
        self.0.extend_from_slice(s.as_bytes());
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    /// `if (primary->length > 4) SetAt(primary, 4, '\0')` — the C keeps the
    /// metastring length but NUL-terminates at 4, so the caller sees 4 chars.
    fn finish(mut self) -> String {
        self.0.truncate(4);
        String::from_utf8(self.0).expect("codes are ASCII by construction")
    }
}

/// Raw Double Metaphone: returns `(primary, secondary)`, each already cut to
/// four characters. Either may be empty. No `None` collapsing happens here —
/// that is the Python layer's job, see [`crate::dmetaphone`].
///
/// Input is bytes because the original operates on a C string; the Python layer
/// only ever hands it ASCII (see [`crate::AsciiError`]).
pub fn dmetaphone_raw(input: &[u8]) -> (String, String) {
    let word = Word::new(input);
    // The C takes strlen(str) *before* padding, and `last` may be -1.
    let length = input.len() as i32;
    let last = length - 1;

    let mut primary = Code::default();
    let mut secondary = Code::default();
    let mut current: i32 = 0;

    // Skip these when at the start of a word.
    if word.string_at(0, 2, &["GN", "KN", "PN", "WR", "PS"]) {
        current += 1;
    }

    // Initial 'X' is pronounced 'Z' e.g. 'Xavier'.
    if word.get_at(0) == b'X' {
        primary.add("S"); // 'Z' maps to 'S'
        secondary.add("S");
        current += 1;
    }

    while primary.len() < 4 || secondary.len() < 4 {
        if current >= length {
            break;
        }

        match word.get_at(current) {
            b'A' | b'E' | b'I' | b'O' | b'U' | b'Y' => {
                if current == 0 {
                    // all init vowels now map to 'A'
                    primary.add("A");
                    secondary.add("A");
                }
                current += 1;
            }

            b'B' => {
                // "-mb", e.g. "dumb", already skipped over...
                primary.add("P");
                secondary.add("P");
                current += if word.get_at(current + 1) == b'B' { 2 } else { 1 };
            }

            // 0xC7 = 'Ç' in Latin-1. Unreachable through the Python API, which
            // encodes as ASCII and raises first; reachable through the Rust API.
            0xC7 => {
                primary.add("S");
                secondary.add("S");
                current += 1;
            }

            b'C' => {
                // various germanic
                if current > 1
                    && !word.is_vowel(current - 2)
                    && word.string_at(current - 1, 3, &["ACH"])
                    && (word.get_at(current + 2) != b'I'
                        && (word.get_at(current + 2) != b'E'
                            || word.string_at(current - 2, 6, &["BACHER", "MACHER"])))
                {
                    primary.add("K");
                    secondary.add("K");
                    current += 2;
                    continue;
                }

                // special case 'caesar'
                if current == 0 && word.string_at(current, 6, &["CAESAR"]) {
                    primary.add("S");
                    secondary.add("S");
                    current += 2;
                    continue;
                }

                // italian 'chianti'
                if word.string_at(current, 4, &["CHIA"]) {
                    primary.add("K");
                    secondary.add("K");
                    current += 2;
                    continue;
                }

                if word.string_at(current, 2, &["CH"]) {
                    // find 'michael'
                    if current > 0 && word.string_at(current, 4, &["CHAE"]) {
                        primary.add("K");
                        secondary.add("X");
                        current += 2;
                        continue;
                    }

                    // greek roots e.g. 'chemistry', 'chorus'
                    if current == 0
                        && (word.string_at(current + 1, 5, &["HARAC", "HARIS"])
                            || word.string_at(current + 1, 3, &["HOR", "HYM", "HIA", "HEM"]))
                        && !word.string_at(0, 5, &["CHORE"])
                    {
                        primary.add("K");
                        secondary.add("K");
                        current += 2;
                        continue;
                    }

                    // germanic, greek, or otherwise 'ch' for 'kh' sound
                    if (word.string_at(0, 4, &["VAN ", "VON "]) || word.string_at(0, 3, &["SCH"]))
                        || word.string_at(current - 2, 6, &["ORCHES", "ARCHIT", "ORCHID"])
                        || word.string_at(current + 2, 1, &["T", "S"])
                        || ((word.string_at(current - 1, 1, &["A", "O", "U", "E"]) || current == 0)
                            // e.g. 'wachtler', 'wechsler', but not 'tichner'
                            && word.string_at(
                                current + 2,
                                1,
                                &["L", "R", "N", "M", "B", "H", "F", "V", "W", " "],
                            ))
                    {
                        primary.add("K");
                        secondary.add("K");
                    } else if current > 0 {
                        if word.string_at(0, 2, &["MC"]) {
                            // e.g. "McHugh"
                            primary.add("K");
                            secondary.add("K");
                        } else {
                            primary.add("X");
                            secondary.add("K");
                        }
                    } else {
                        primary.add("X");
                        secondary.add("X");
                    }
                    current += 2;
                    continue;
                }

                // e.g. 'czerny'
                if word.string_at(current, 2, &["CZ"]) && !word.string_at(current - 2, 4, &["WICZ"])
                {
                    primary.add("S");
                    secondary.add("X");
                    current += 2;
                    continue;
                }

                // e.g. 'focaccia'
                if word.string_at(current + 1, 3, &["CIA"]) {
                    primary.add("X");
                    secondary.add("X");
                    current += 3;
                    continue;
                }

                // UPSTREAM BUG (DECISIONS.md 04): in Aubrey's original the `else`
                // below binds to the *inner* `if` — Pierce's rule is the fallback
                // for a "CC" that is not followed by I/E/H. yougov's copy wrapped
                // the inner `if` in braces, so the `else` now binds to the outer
                // `if` and fires for every C that is *not* "CC". Consequence: the
                // CK/CG/CQ, CI/CE/CY and default arms below are unreachable, every
                // soft C is coded "K", and the cursor over-advances by one,
                // swallowing the following letter. 'cent' -> "KNT", not "SNT".
                // Preserved deliberately; this is the shipped behavior.
                if word.string_at(current, 2, &["CC"])
                    && !(current == 1 && word.get_at(0) == b'M')
                {
                    // 'bellocchio' but not 'bacchus'
                    if word.string_at(current + 2, 1, &["I", "E", "H"])
                        && !word.string_at(current + 2, 2, &["HU"])
                    {
                        // 'accident', 'accede', 'succeed'
                        if (current == 1 && word.get_at(current - 1) == b'A')
                            || word.string_at(current - 1, 5, &["UCCEE", "UCCES"])
                        {
                            primary.add("KS");
                            secondary.add("KS");
                        } else {
                            // 'bacci', 'bertucci', other italian
                            primary.add("X");
                            secondary.add("X");
                        }
                        current += 3;
                        continue;
                    }
                } else {
                    // Pierce's rule
                    primary.add("K");
                    secondary.add("K");
                    current += 2;
                    continue;
                }

                // --- unreachable in practice, kept for structural fidelity ---
                if word.string_at(current, 2, &["CK", "CG", "CQ"]) {
                    primary.add("K");
                    secondary.add("K");
                    current += 2;
                    continue;
                }

                if word.string_at(current, 2, &["CI", "CE", "CY"]) {
                    // italian vs. english
                    if word.string_at(current, 3, &["CIO", "CIE", "CIA"]) {
                        primary.add("S");
                        secondary.add("X");
                    } else {
                        primary.add("S");
                        secondary.add("S");
                    }
                    current += 2;
                    continue;
                }

                // else
                primary.add("K");
                secondary.add("K");

                // name sent in 'mac caffrey', 'mac gregor'
                if word.string_at(current + 1, 2, &[" C", " Q", " G"]) {
                    current += 3;
                } else if word.string_at(current + 1, 1, &["C", "K", "Q"])
                    && !word.string_at(current + 1, 2, &["CE", "CI"])
                {
                    current += 2;
                } else {
                    current += 1;
                }
            }

            b'D' => {
                if word.string_at(current, 2, &["DG"]) {
                    if word.string_at(current + 2, 1, &["I", "E", "Y"]) {
                        // e.g. 'edge'
                        primary.add("J");
                        secondary.add("J");
                        current += 3;
                    } else {
                        // e.g. 'edgar'
                        primary.add("TK");
                        secondary.add("TK");
                        current += 2;
                    }
                    continue;
                }

                if word.string_at(current, 2, &["DT", "DD"]) {
                    primary.add("T");
                    secondary.add("T");
                    current += 2;
                    continue;
                }

                // else
                primary.add("T");
                secondary.add("T");
                current += 1;
            }

            b'F' => {
                current += if word.get_at(current + 1) == b'F' { 2 } else { 1 };
                primary.add("F");
                secondary.add("F");
            }

            b'G' => {
                if word.get_at(current + 1) == b'H' {
                    if current > 0 && !word.is_vowel(current - 1) {
                        primary.add("K");
                        secondary.add("K");
                        current += 2;
                        continue;
                    }

                    if current < 3 {
                        // 'ghislane', 'ghiradelli'
                        if current == 0 {
                            if word.get_at(current + 2) == b'I' {
                                primary.add("J");
                                secondary.add("J");
                            } else {
                                primary.add("K");
                                secondary.add("K");
                            }
                            current += 2;
                            continue;
                        }
                    }

                    // Parker's rule (with some further refinements) - e.g. 'hugh'
                    if (current > 1 && word.string_at(current - 2, 1, &["B", "H", "D"]))
                        // e.g. 'bough'
                        || (current > 2 && word.string_at(current - 3, 1, &["B", "H", "D"]))
                        // e.g. 'broughton'
                        || (current > 3 && word.string_at(current - 4, 1, &["B", "H"]))
                    {
                        current += 2;
                        continue;
                    } else {
                        // e.g. 'laugh', 'McLaughlin', 'cough', 'gough', 'rough', 'tough'
                        if current > 2
                            && word.get_at(current - 1) == b'U'
                            && word.string_at(current - 3, 1, &["C", "G", "L", "R", "T"])
                        {
                            primary.add("F");
                            secondary.add("F");
                        } else if current > 0 && word.get_at(current - 1) != b'I' {
                            primary.add("K");
                            secondary.add("K");
                        }
                        current += 2;
                        continue;
                    }
                }

                if word.get_at(current + 1) == b'N' {
                    if current == 1 && word.is_vowel(0) && !word.slavo_germanic() {
                        primary.add("KN");
                        secondary.add("N");
                    } else if !word.string_at(current + 2, 2, &["EY"])
                        && word.get_at(current + 1) != b'Y'
                        && !word.slavo_germanic()
                    {
                        // not e.g. 'cagney'
                        primary.add("N");
                        secondary.add("KN");
                    } else {
                        primary.add("KN");
                        secondary.add("KN");
                    }
                    current += 2;
                    continue;
                }

                // 'tagliaro'
                if word.string_at(current + 1, 2, &["LI"]) && !word.slavo_germanic() {
                    primary.add("KL");
                    secondary.add("L");
                    current += 2;
                    continue;
                }

                // -ges-, -gep-, -gel-, -gie- at beginning
                if current == 0
                    && (word.get_at(current + 1) == b'Y'
                        || word.string_at(
                            current + 1,
                            2,
                            &["ES", "EP", "EB", "EL", "EY", "IB", "IL", "IN", "IE", "EI", "ER"],
                        ))
                {
                    primary.add("K");
                    secondary.add("J");
                    current += 2;
                    continue;
                }

                // -ger-, -gy-
                if (word.string_at(current + 1, 2, &["ER"]) || word.get_at(current + 1) == b'Y')
                    && !word.string_at(0, 6, &["DANGER", "RANGER", "MANGER"])
                    && !word.string_at(current - 1, 1, &["E", "I"])
                    && !word.string_at(current - 1, 3, &["RGY", "OGY"])
                {
                    primary.add("K");
                    secondary.add("J");
                    current += 2;
                    continue;
                }

                // italian e.g. 'biaggi'
                if word.string_at(current + 1, 1, &["E", "I", "Y"])
                    || word.string_at(current - 1, 4, &["AGGI", "OGGI"])
                {
                    // obvious germanic
                    if (word.string_at(0, 4, &["VAN ", "VON "]) || word.string_at(0, 3, &["SCH"]))
                        || word.string_at(current + 1, 2, &["ET"])
                    {
                        primary.add("K");
                        secondary.add("K");
                    } else if word.string_at(current + 1, 4, &["IER "]) {
                        // always soft if french ending
                        primary.add("J");
                        secondary.add("J");
                    } else {
                        primary.add("J");
                        secondary.add("K");
                    }
                    current += 2;
                    continue;
                }

                current += if word.get_at(current + 1) == b'G' { 2 } else { 1 };
                primary.add("K");
                secondary.add("K");
            }

            b'H' => {
                // only keep if first & before vowel or btw. 2 vowels
                if (current == 0 || word.is_vowel(current - 1)) && word.is_vowel(current + 1) {
                    primary.add("H");
                    secondary.add("H");
                    current += 2;
                } else {
                    // also takes care of 'HH'
                    current += 1;
                }
            }

            b'J' => {
                // obvious spanish, 'jose', 'san jacinto'
                if word.string_at(current, 4, &["JOSE"]) || word.string_at(0, 4, &["SAN "]) {
                    if (current == 0 && word.get_at(current + 4) == b' ')
                        || word.string_at(0, 4, &["SAN "])
                    {
                        primary.add("H");
                        secondary.add("H");
                    } else {
                        primary.add("J");
                        secondary.add("H");
                    }
                    current += 1;
                    continue;
                }

                if current == 0 && !word.string_at(current, 4, &["JOSE"]) {
                    primary.add("J"); // Yankelovich/Jankelowicz
                    secondary.add("A");
                } else if word.is_vowel(current - 1)
                    && !word.slavo_germanic()
                    && (word.get_at(current + 1) == b'A' || word.get_at(current + 1) == b'O')
                {
                    // spanish pron. of e.g. 'bajador'
                    primary.add("J");
                    secondary.add("H");
                } else if current == last {
                    primary.add("J");
                    secondary.add("");
                } else if !word.string_at(
                    current + 1,
                    1,
                    &["L", "T", "K", "S", "N", "M", "B", "Z"],
                ) && !word.string_at(current - 1, 1, &["S", "K", "L"])
                {
                    primary.add("J");
                    secondary.add("J");
                }

                current += if word.get_at(current + 1) == b'J' { 2 } else { 1 };
            }

            b'K' => {
                current += if word.get_at(current + 1) == b'K' { 2 } else { 1 };
                primary.add("K");
                secondary.add("K");
            }

            b'L' => {
                if word.get_at(current + 1) == b'L' {
                    // spanish e.g. 'cabrillo', 'gallegos'
                    if (current == length - 3
                        && word.string_at(current - 1, 4, &["ILLO", "ILLA", "ALLE"]))
                        || ((word.string_at(last - 1, 2, &["AS", "OS"])
                            || word.string_at(last, 1, &["A", "O"]))
                            && word.string_at(current - 1, 4, &["ALLE"]))
                    {
                        primary.add("L");
                        secondary.add("");
                        current += 2;
                        continue;
                    }
                    current += 2;
                } else {
                    current += 1;
                }
                primary.add("L");
                secondary.add("L");
            }

            b'M' => {
                if (word.string_at(current - 1, 3, &["UMB"])
                    && (current + 1 == last || word.string_at(current + 2, 2, &["ER"])))
                    // 'dumb', 'thumb'
                    || word.get_at(current + 1) == b'M'
                {
                    current += 2;
                } else {
                    current += 1;
                }
                primary.add("M");
                secondary.add("M");
            }

            b'N' => {
                current += if word.get_at(current + 1) == b'N' { 2 } else { 1 };
                primary.add("N");
                secondary.add("N");
            }

            // 0xD1 = 'Ñ' in Latin-1. Unreachable through the Python API.
            0xD1 => {
                current += 1;
                primary.add("N");
                secondary.add("N");
            }

            b'P' => {
                if word.get_at(current + 1) == b'H' {
                    primary.add("F");
                    secondary.add("F");
                    current += 2;
                    continue;
                }

                // also account for "campbell", "raspberry"
                current += if word.string_at(current + 1, 1, &["P", "B"]) { 2 } else { 1 };
                primary.add("P");
                secondary.add("P");
            }

            b'Q' => {
                current += if word.get_at(current + 1) == b'Q' { 2 } else { 1 };
                primary.add("K");
                secondary.add("K");
            }

            b'R' => {
                // french e.g. 'rogier', but exclude 'hochmeier'
                if current == last
                    && !word.slavo_germanic()
                    && word.string_at(current - 2, 2, &["IE"])
                    && !word.string_at(current - 4, 2, &["ME", "MA"])
                {
                    primary.add("");
                    secondary.add("R");
                } else {
                    primary.add("R");
                    secondary.add("R");
                }

                current += if word.get_at(current + 1) == b'R' { 2 } else { 1 };
            }

            b'S' => {
                // special cases 'island', 'isle', 'carlisle', 'carlysle'
                if word.string_at(current - 1, 3, &["ISL", "YSL"]) {
                    current += 1;
                    continue;
                }

                // special case 'sugar-'
                if current == 0 && word.string_at(current, 5, &["SUGAR"]) {
                    primary.add("X");
                    secondary.add("S");
                    current += 1;
                    continue;
                }

                if word.string_at(current, 2, &["SH"]) {
                    // germanic
                    if word.string_at(current + 1, 4, &["HEIM", "HOEK", "HOLM", "HOLZ"]) {
                        primary.add("S");
                        secondary.add("S");
                    } else {
                        primary.add("X");
                        secondary.add("X");
                    }
                    current += 2;
                    continue;
                }

                // italian & armenian
                if word.string_at(current, 3, &["SIO", "SIA"])
                    || word.string_at(current, 4, &["SIAN"])
                {
                    if !word.slavo_germanic() {
                        primary.add("S");
                        secondary.add("X");
                    } else {
                        primary.add("S");
                        secondary.add("S");
                    }
                    current += 3;
                    continue;
                }

                // german & anglicisations, e.g. 'smith' matches 'schmidt',
                // 'snider' matches 'schneider'; also -sz- in slavic languages
                if (current == 0 && word.string_at(current + 1, 1, &["M", "N", "L", "W"]))
                    || word.string_at(current + 1, 1, &["Z"])
                {
                    primary.add("S");
                    secondary.add("X");
                    current += if word.string_at(current + 1, 1, &["Z"]) { 2 } else { 1 };
                    continue;
                }

                if word.string_at(current, 2, &["SC"]) {
                    // Schlesinger's rule.
                    //
                    // UPSTREAM BUG (DECISIONS.md 05): the brace closing this
                    // `if` is missing in yougov's copy, so the "SC" + I/E/Y and
                    // the default "SK" arms below sit *inside* it — and both
                    // arms of the inner if/else already break. They are dead
                    // code. Any "SC" not followed by H therefore falls out of
                    // the whole block to the generic S handling below.
                    // 'science' -> "SKNK", not "SNS". Preserved deliberately.
                    if word.get_at(current + 2) == b'H' {
                        // dutch origin, e.g. 'school', 'schooner'
                        if word.string_at(current + 3, 2, &["OO", "ER", "EN", "UY", "ED", "EM"]) {
                            // 'schermerhorn', 'schenker'
                            if word.string_at(current + 3, 2, &["ER", "EN"]) {
                                primary.add("X");
                                secondary.add("SK");
                            } else {
                                primary.add("SK");
                                secondary.add("SK");
                            }
                            current += 3;
                            continue;
                        } else {
                            if current == 0 && !word.is_vowel(3) && word.get_at(3) != b'W' {
                                primary.add("X");
                                secondary.add("S");
                            } else {
                                primary.add("X");
                                secondary.add("X");
                            }
                            current += 3;
                            continue;
                        }

                        // --- dead code in the original, kept for fidelity ---
                        // if word.string_at(current + 2, 1, &["I", "E", "Y"]) {
                        //     primary.add("S"); secondary.add("S");
                        //     current += 3; continue;
                        // }
                        // primary.add("SK"); secondary.add("SK");
                        // current += 3; continue;
                    }
                }

                // french e.g. 'resnais', 'artois'
                if current == last && word.string_at(current - 2, 2, &["AI", "OI"]) {
                    primary.add("");
                    secondary.add("S");
                } else {
                    primary.add("S");
                    secondary.add("S");
                }

                current += if word.string_at(current + 1, 1, &["S", "Z"]) { 2 } else { 1 };
            }

            b'T' => {
                if word.string_at(current, 4, &["TION"]) {
                    primary.add("X");
                    secondary.add("X");
                    current += 3;
                    continue;
                }

                if word.string_at(current, 3, &["TIA", "TCH"]) {
                    primary.add("X");
                    secondary.add("X");
                    current += 3;
                    continue;
                }

                if word.string_at(current, 2, &["TH"]) || word.string_at(current, 3, &["TTH"]) {
                    // special case 'thomas', 'thames' or germanic
                    if word.string_at(current + 2, 2, &["OM", "AM"])
                        || word.string_at(0, 4, &["VAN ", "VON "])
                        || word.string_at(0, 3, &["SCH"])
                    {
                        primary.add("T");
                        secondary.add("T");
                    } else {
                        primary.add("0"); // yes, zero
                        secondary.add("T");
                    }
                    current += 2;
                    continue;
                }

                current += if word.string_at(current + 1, 1, &["T", "D"]) { 2 } else { 1 };
                primary.add("T");
                secondary.add("T");
            }

            b'V' => {
                current += if word.get_at(current + 1) == b'V' { 2 } else { 1 };
                primary.add("F");
                secondary.add("F");
            }

            b'W' => {
                // can also be in middle of word
                if word.string_at(current, 2, &["WR"]) {
                    primary.add("R");
                    secondary.add("R");
                    current += 2;
                    continue;
                }

                if current == 0
                    && (word.is_vowel(current + 1) || word.string_at(current, 2, &["WH"]))
                {
                    if word.is_vowel(current + 1) {
                        // Wasserman should match Vasserman
                        primary.add("A");
                        secondary.add("F");
                    } else {
                        // need Uomo to match Womo
                        primary.add("A");
                        secondary.add("A");
                    }
                }

                // Arnow should match Arnoff
                if (current == last && word.is_vowel(current - 1))
                    || word.string_at(current - 1, 5, &["EWSKI", "EWSKY", "OWSKI", "OWSKY"])
                    || word.string_at(0, 3, &["SCH"])
                {
                    primary.add("");
                    secondary.add("F");
                    current += 1;
                    continue;
                }

                // polish e.g. 'filipowicz'
                if word.string_at(current, 4, &["WICZ", "WITZ"]) {
                    primary.add("TS");
                    secondary.add("FX");
                    current += 4;
                    continue;
                }

                // else skip it
                current += 1;
            }

            b'X' => {
                // french e.g. 'breaux'
                if !(current == last
                    && (word.string_at(current - 3, 3, &["IAU", "EAU"])
                        || word.string_at(current - 2, 2, &["AU", "OU"])))
                {
                    primary.add("KS");
                    secondary.add("KS");
                }

                current += if word.string_at(current + 1, 1, &["C", "X"]) { 2 } else { 1 };
            }

            b'Z' => {
                // chinese pinyin e.g. 'zhao'
                if word.get_at(current + 1) == b'H' {
                    primary.add("J");
                    secondary.add("J");
                    current += 2;
                    continue;
                } else if word.string_at(current + 1, 2, &["ZO", "ZI", "ZA"])
                    || (word.slavo_germanic()
                        && (current > 0 && word.get_at(current - 1) != b'T'))
                {
                    primary.add("S");
                    secondary.add("TS");
                } else {
                    primary.add("S");
                    secondary.add("S");
                }

                current += if word.get_at(current + 1) == b'Z' { 2 } else { 1 };
            }

            _ => {
                current += 1;
            }
        }
    }

    (primary.finish(), secondary.finish())
}
