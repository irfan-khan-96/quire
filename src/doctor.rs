//! `quire doctor`: report which external tools are available.

use std::process::Command;

use anstyle::{AnsiColor, Style};

use crate::project::quire_home;
use crate::shell;

const OK: Style = AnsiColor::Green.on_default().bold();
const MISSING: Style = AnsiColor::Red.on_default().bold();
const OPTIONAL: Style = AnsiColor::Yellow.on_default().bold();

struct Tool {
    program: &'static str,
    version_arg: &'static str,
    purpose: &'static str,
    required: bool,
}

const TOOLS: &[Tool] = &[
    Tool { program: "latexmk", version_arg: "-v", purpose: "runs the build", required: true },
    Tool { program: "perl", version_arg: "-v", purpose: "needed by latexmk", required: true },
    Tool { program: "pdflatex", version_arg: "--version", purpose: "engine = \"pdflatex\"", required: false },
    Tool { program: "xelatex", version_arg: "--version", purpose: "engine = \"xelatex\"", required: false },
    Tool { program: "lualatex", version_arg: "--version", purpose: "engine = \"lualatex\"", required: false },
    Tool { program: "kpsewhich", version_arg: "--version", purpose: "checks distribution packages", required: false },
    Tool { program: "biber", version_arg: "--version", purpose: "biblatex bibliographies", required: false },
    Tool { program: "git", version_arg: "--version", purpose: "git dependencies, `quire new`", required: false },
];

/// Returns false if a required tool is missing.
pub fn doctor() -> bool {
    let mut healthy = true;
    for tool in TOOLS {
        let output = Command::new(tool.program).arg(tool.version_arg).output();
        match output {
            Ok(out) => {
                let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                let version = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
                shell::outln!("  {OK}ok{OK:#}       {:<13} {version}", tool.program);
            }
            Err(_) if tool.required => {
                healthy = false;
                shell::outln!("  {MISSING}missing{MISSING:#}  {:<13} {}", tool.program, tool.purpose);
            }
            Err(_) => {
                shell::outln!("  {OPTIONAL}missing{OPTIONAL:#}  {:<13} optional: {}", tool.program, tool.purpose);
            }
        }
    }
    let mylatexformat = Command::new("kpsewhich")
        .arg("mylatexformat.ltx")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|p| !p.is_empty());
    match mylatexformat {
        Some(path) => shell::outln!("  {OK}ok{OK:#}       {:<13} {path}", "mylatexformat"),
        None => shell::outln!(
            "  {OPTIONAL}missing{OPTIONAL:#}  {:<13} optional: preamble caching (faster pdflatex rebuilds)",
            "mylatexformat"
        ),
    }
    if let Ok(home) = quire_home() {
        shell::outln!("\n  quire home: {}", home.display());
    }
    if !healthy {
        shell::outln!(
            "\n  Install TeX Live (https://tug.org/texlive/) or MiKTeX (https://miktex.org/), which include latexmk."
        );
    }
    healthy
}
