//! Cargo-style terminal output: right-aligned green status verbs, warnings and errors.

use std::fmt::Display;

use anstyle::{AnsiColor, Style};

const STATUS: Style = AnsiColor::Green.on_default().bold();
const WARN: Style = AnsiColor::Yellow.on_default().bold();
const ERROR: Style = AnsiColor::Red.on_default().bold();
const NOTE: Style = AnsiColor::Cyan.on_default().bold();

/// `   Compiling thesis (pdflatex)`
pub fn status(verb: &str, msg: impl Display) {
    anstream::eprintln!("{STATUS}{verb:>12}{STATUS:#} {msg}");
}

pub fn warn(msg: impl Display) {
    anstream::eprintln!("{WARN}warning{WARN:#}: {msg}");
}

pub fn note(msg: impl Display) {
    anstream::eprintln!("{NOTE}note{NOTE:#}: {msg}");
}

/// A TeX error extracted from a log file (not a quire failure).
pub fn tex_error(msg: impl Display) {
    anstream::eprintln!("{ERROR}error{ERROR:#}: {msg}");
}

pub fn error(err: &anyhow::Error) {
    anstream::eprintln!("{ERROR}error{ERROR:#}: {err}");
    let mut causes = err.chain().skip(1).peekable();
    if causes.peek().is_some() {
        anstream::eprintln!("\nCaused by:");
        for cause in causes {
            for line in cause.to_string().lines() {
                anstream::eprintln!("  {line}");
            }
        }
    }
}
