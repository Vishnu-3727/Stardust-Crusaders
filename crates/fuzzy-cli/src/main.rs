//! `fuzzy` — line-protocol front end for the `fuzzy` crate.
//!
//! One binary serves all three consumers: the Python compatibility shim that
//! runs the original test suite, the differential fuzzer, and the benchmark
//! harness. Stdin in, stdout out, one line each way, flushed per line.
//!
//! Request:  `ALGO<TAB>SIZE<TAB>WORD`   (SIZE is ignored by NYSIIS but required)
//! Response: `OK<TAB>...` or `ERR<TAB>message`
//!
//!   SOUNDEX     -> `OK<TAB>code`
//!   NYSIIS      -> `OK<TAB>code`
//!   DMETAPHONE  -> `OK<TAB>primary<TAB>secondary`, absent codes as `NULL`
//!
//! `NULL` is unambiguous: metaphone codes are drawn from `AEFHJKLMNPRSTX0`,
//! so no real code can spell it.
//!
//! WORD may not contain a newline — a line protocol cannot carry one. The
//! library API has no such restriction; nothing in the original test suite or
//! the fuzz corpus needs it.

use std::io::{self, BufRead, Write};

fn handle(line: &str) -> String {
    let mut parts = line.splitn(3, '\t');
    let (Some(algo), Some(size), Some(word)) = (parts.next(), parts.next(), parts.next()) else {
        return "ERR\tmalformed request, want ALGO<TAB>SIZE<TAB>WORD".to_string();
    };

    let Ok(size) = size.parse::<usize>() else {
        return format!("ERR\tbad size {size:?}");
    };

    match algo {
        "SOUNDEX" => match fuzzy::soundex(word, size) {
            Ok(code) => format!("OK\t{code}"),
            Err(e) => format!("ERR\tUnicodeEncodeError: {e}"),
        },
        "NYSIIS" => format!("OK\t{}", fuzzy::nysiis(word)),
        "DMETAPHONE" => match fuzzy::dmetaphone(word, size) {
            Ok((primary, secondary)) => format!(
                "OK\t{}\t{}",
                primary.as_deref().unwrap_or("NULL"),
                secondary.as_deref().unwrap_or("NULL")
            ),
            Err(e) => format!("ERR\tUnicodeEncodeError: {e}"),
        },
        other => format!("ERR\tunknown algorithm {other:?}"),
    }
}

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    if let Some(flag) = args.next() {
        if flag == "--help" || flag == "-h" {
            print!("{}", include_str!("usage.txt"));
            return Ok(());
        }
        eprintln!("unknown argument {flag:?}; try --help");
        std::process::exit(2);
    }

    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    for line in stdin.lines() {
        let line = line?;
        writeln!(stdout, "{}", handle(line.trim_end_matches('\r')))?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::handle;

    #[test]
    fn protocol() {
        assert_eq!(handle("SOUNDEX\t4\tfuzzy"), "OK\tF200");
        assert_eq!(handle("NYSIIS\t0\tfuzzy"), "OK\tFASY");
        assert_eq!(handle("DMETAPHONE\t0\tmayer"), "OK\tMR\tNULL");
        assert_eq!(handle("DMETAPHONE\t0\tsmith"), "OK\tSM0\tXMT");
        assert_eq!(handle("DMETAPHONE\t0\t"), "OK\tNULL\tNULL");
        assert!(handle("SOUNDEX\t4\tJéroboam").starts_with("ERR\tUnicodeEncodeError"));
        assert!(handle("NOPE\t0\tx").starts_with("ERR\tunknown algorithm"));
        assert!(handle("SOUNDEX\tx\ty").starts_with("ERR\tbad size"));
        assert!(handle("SOUNDEX").starts_with("ERR\tmalformed"));
    }
}
