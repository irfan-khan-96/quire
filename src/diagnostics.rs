//! Turning a TeX log into structured diagnostics, and reporting them either
//! for humans (stderr) or as JSON lines for editors and CI (stdout), the way
//! `cargo --message-format=json` does.

use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use crate::project::tex_path;
use crate::shell;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum MessageFormat {
    #[default]
    Human,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    /// Source file, absolute when it could be resolved.
    pub file: Option<String>,
    pub line: Option<u32>,
    /// The `l.NN ...` lines TeX prints to show where it stopped.
    pub context: Vec<String>,
}

impl Diagnostic {
    /// Human-readable form, as printed after `error: `.
    pub fn rendered(&self) -> String {
        let mut out = match (&self.file, self.line) {
            (Some(file), Some(line)) => format!("{file}:{line}: {}", self.message),
            _ => self.message.clone(),
        };
        for line in &self.context {
            out.push_str("\n  ");
            out.push_str(line);
        }
        out
    }

    /// Make a relative file path absolute (TeX reports paths relative to the
    /// directory it ran in).
    pub fn resolve_file(&mut self, base: &Path) {
        if let Some(file) = &self.file {
            let relative = file.strip_prefix("./").unwrap_or(file);
            if !Path::new(relative).is_absolute() {
                self.file = Some(tex_path(&base.join(relative)));
            }
        }
    }
}

/// Splits `./chapter.tex:12: Undefined control sequence.` (the
/// `-file-line-error` format) into file, line and message.
fn split_file_line(line: &str) -> Option<(&str, u32, &str)> {
    line.match_indices(':').find_map(|(i, _)| {
        let rest = &line[i + 1..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || !rest[digits..].starts_with(": ") || i == 0 {
            return None;
        }
        let number = rest[..digits].parse().ok()?;
        Some((&line[..i], number, &rest[digits + 2..]))
    })
}

/// `l.12 \foo` -> 12
fn context_line_number(line: &str) -> Option<u32> {
    let rest = line.strip_prefix("l.")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Errors in a TeX log, each with the `l.NN ...` context lines that follow it.
pub fn parse_errors(log: &str) -> Vec<Diagnostic> {
    let lines: Vec<&str> = log.lines().collect();
    let mut errors = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let noise = line.contains("Emergency stop") || line.contains("==> Fatal error occurred");
        let parsed = if noise {
            None
        } else if let Some(message) = line.strip_prefix("! ") {
            Some(Diagnostic {
                level: Level::Error,
                message: message.to_string(),
                file: None,
                line: None,
                context: vec![],
            })
        } else {
            split_file_line(line).map(|(file, number, message)| Diagnostic {
                level: Level::Error,
                message: message.to_string(),
                file: Some(file.to_string()),
                line: Some(number),
                context: vec![],
            })
        };
        let Some(mut diagnostic) = parsed else {
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < lines.len() && j <= i + 6 && !lines[j].trim().is_empty() {
            if let Some(number) = context_line_number(lines[j]) {
                diagnostic.line.get_or_insert(number);
                diagnostic.context.push(lines[j].to_string());
                if let Some(next) = lines.get(j + 1).filter(|l| !l.trim().is_empty()) {
                    diagnostic.context.push(next.to_string());
                }
                break;
            }
            j += 1;
        }
        errors.push(diagnostic);
        i = j.max(i + 1);
    }
    errors
}

/// LaTeX and package warnings. A warning runs until the next blank line.
pub fn parse_warnings(log: &str) -> Vec<Diagnostic> {
    let lines: Vec<&str> = log.lines().collect();
    let mut warnings = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(start) = lines[i].find("Warning:") else {
            i += 1;
            continue;
        };
        let prefix = &lines[i][..start];
        let is_warning = prefix.starts_with("LaTeX ") || prefix.starts_with("Package ") || prefix.starts_with("Class ");
        if !is_warning {
            i += 1;
            continue;
        }
        let mut message = lines[i].trim().to_string();
        let mut j = i + 1;
        while j < lines.len() && j <= i + 4 && !lines[j].trim().is_empty() {
            message.push(' ');
            message.push_str(lines[j].trim());
            j += 1;
        }
        let line = message
            .find("on input line ")
            .and_then(|k| message[k + 14..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok());
        warnings.push(Diagnostic { level: Level::Warning, message, file: None, line, context: vec![] });
        i = j;
    }
    warnings
}

/// Where build output goes.
#[derive(Clone, Copy)]
pub struct Reporter {
    pub format: MessageFormat,
}

impl Reporter {
    pub fn is_json(self) -> bool {
        self.format == MessageFormat::Json
    }

    fn emit(self, value: Value) {
        println!("{value}");
    }

    pub fn diagnostic(self, document: &str, profile: &str, diagnostic: &Diagnostic) {
        if self.is_json() {
            let mut message = serde_json::to_value(diagnostic).expect("diagnostic serializes");
            message["rendered"] = Value::String(diagnostic.rendered());
            self.emit(json!({
                "reason": "compiler-message",
                "document": document,
                "profile": profile,
                "message": message,
            }));
        } else if diagnostic.level == Level::Error {
            shell::tex_error(diagnostic.rendered());
        }
    }

    pub fn artifact(self, document: &str, profile: &str, pdf: &Path, log: &Path) {
        if self.is_json() {
            self.emit(json!({
                "reason": "document-artifact",
                "document": document,
                "profile": profile,
                "pdf": tex_path(pdf),
                "log": tex_path(log),
            }));
        }
    }

    pub fn finished(self, success: bool) {
        if self.is_json() {
            self.emit(json!({ "reason": "build-finished", "success": success }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_errors() {
        let log = "This is pdfTeX\n./main.tex:5: Undefined control sequence.\nl.5 \\foo\n         bar\n\n! LaTeX Error: File `x.sty' not found.\n\nType X to quit.\nl.3 \\usepackage\n\n./main.tex:3:  ==> Fatal error occurred, no output PDF file produced!\n";
        let errors = parse_errors(log);
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].file.as_deref(), Some("./main.tex"));
        assert_eq!(errors[0].line, Some(5));
        assert_eq!(errors[0].message, "Undefined control sequence.");
        assert_eq!(errors[0].rendered(), "./main.tex:5: Undefined control sequence.\n  l.5 \\foo\n           bar");
        assert_eq!(errors[1].message, "LaTeX Error: File `x.sty' not found.");
        assert_eq!(errors[1].file, None);
    }

    #[test]
    fn windows_drive_letters_are_not_line_numbers() {
        let (file, line, message) = split_file_line("C:/work/ch1.tex:42: Missing $ inserted.").unwrap();
        assert_eq!((file, line, message), ("C:/work/ch1.tex", 42, "Missing $ inserted."));
        assert!(split_file_line("Output written on C:/x.pdf (1 page).").is_none());
    }

    #[test]
    fn parses_multiline_warnings() {
        let log = "LaTeX Warning: Reference `fig:a' on page 1 undefined on input line 12.\n\nPackage hyperref Warning: Token not allowed in a PDF string (Unicode):\n(hyperref)                removing `math shift' on input line 30.\n\nNo Warning: here\n";
        let warnings = parse_warnings(log);
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0].line, Some(12));
        assert!(warnings[1].message.contains("removing `math shift'"));
        assert_eq!(warnings[1].line, Some(30));
    }

    #[test]
    fn resolves_relative_files() {
        let mut d = Diagnostic {
            level: Level::Error,
            message: "x".into(),
            file: Some("./ch/a.tex".into()),
            line: Some(1),
            context: vec![],
        };
        d.resolve_file(Path::new("/proj/src"));
        assert_eq!(d.file.as_deref(), Some("/proj/src/ch/a.tex"));
    }
}
