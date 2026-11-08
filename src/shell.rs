//! Cargo-style terminal output: right-aligned green status verbs, warnings and errors.

use std::fmt::Display;

use anstyle::{AnsiColor, Style};

const STATUS: Style = AnsiColor::Green.on_default().bold();
const WARN: Style = AnsiColor::Yellow.on_default().bold();
const ERROR: Style = AnsiColor::Red.on_default().bold();
const NOTE: Style = AnsiColor::Cyan.on_default().bold();

/// Write a line to stdout. If the reader has gone away (`quire doctor | head`),
/// exit quietly instead of panicking the way `println!` does.
pub fn out(args: std::fmt::Arguments) {
    use std::io::{ErrorKind, Write};
    let mut stdout = anstream::stdout().lock();
    if let Err(e) = writeln!(stdout, "{args}").and_then(|()| stdout.flush())
        && e.kind() == ErrorKind::BrokenPipe
    {
        std::process::exit(0);
    }
}

/// `println!` for stdout output that may be piped: see [`out`].
macro_rules! outln {
    ($($arg:tt)*) => {
        $crate::shell::out(format_args!($($arg)*))
    };
}
pub(crate) use outln;

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
