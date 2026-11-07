//! Compiling a document: writes a small driver file that applies the profile,
//! runs latexmk with the dependency search path, and summarizes the TeX log.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};

use crate::manifest::Profile;
use crate::project::{Project, tex_path};
use crate::resolve::{Resolution, Source};
use crate::shell;

pub struct BuildRequest {
    pub profile: String,
    pub verbose: bool,
}

/// Build the project's document and return the path of the PDF.
pub fn build(project: &Project, resolution: &Resolution, req: &BuildRequest) -> Result<PathBuf> {
    let doc = project.manifest.document.as_ref().ok_or_else(|| {
        anyhow!("`{}` is a package without a [document] section, so there is nothing to build", project.manifest.name())
    })?;
    let profile = project.manifest.profile(&req.profile)?;

    let main = project.root.join(&doc.main);
    if !main.is_file() {
        bail!("main file `{}` does not exist (set `document.main` in quire.toml)", project.display(&main));
    }
    let main_dir = main.parent().unwrap();
    let main_file = main.file_name().unwrap().to_string_lossy().to_string();

    let out_dir = project.target_dir().join(&profile.name);
    fs::create_dir_all(&out_dir).with_context(|| format!("failed to create `{}`", out_dir.display()))?;

    check_distribution_deps(resolution);

    let source = fs::read_to_string(&main).with_context(|| format!("failed to read `{}`", main.display()))?;
    let driver = out_dir.join("quire-driver.tex");
    let search_dirs = resolution.search_dirs();
    write_if_changed(&driver, &driver_source(&profile, &main_file, &source, &search_dirs))?;

    let mut cmd = Command::new("latexmk");
    cmd.current_dir(main_dir)
        .arg(doc.engine.latexmk_flag())
        .args(["-interaction=nonstopmode", "-halt-on-error", "-file-line-error"])
        .arg(format!("-outdir={}", tex_path(&out_dir)))
        .arg(format!("-jobname={}", doc.name));
    if profile.synctex {
        cmd.arg("-synctex=1");
    }
    if profile.shell_escape {
        cmd.arg("-shell-escape");
    }
    cmd.args(&project.manifest.build.latexmk_args).args(&profile.latexmk_args);
    cmd.arg(tex_path(&driver));
    for (var, value) in resolution.search_env() {
        cmd.env(var, value);
    }

    shell::status("Compiling", format!("{} ({}, profile `{}`)", doc.name, doc.engine, profile.name));
    let started = Instant::now();
    let output = run(&mut cmd, req.verbose)?;

    let log_path = out_dir.join(format!("{}.log", doc.name));
    let log = fs::read(&log_path).ok().map(|b| String::from_utf8_lossy(&b).into_owned());

    if !output.status.success() {
        let errors = log.as_deref().map(log_errors).unwrap_or_default();
        if errors.is_empty() {
            if !req.verbose {
                let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
                let lines: Vec<&str> = text.lines().collect();
                for line in &lines[lines.len().saturating_sub(25)..] {
                    eprintln!("  {line}");
                }
            }
        } else {
            for error in errors.iter().take(5) {
                shell::tex_error(error);
            }
            if errors.len() > 5 {
                shell::note(format!("{} more errors not shown", errors.len() - 5));
            }
        }
        if log.is_some() {
            shell::note(format!("full log: {}", project.display(&log_path)));
        }
        bail!("could not compile `{}`", doc.name);
    }

    if let Some(log) = &log {
        report_warnings(project, log, &log_path, req.verbose);
    }

    let pdf = out_dir.join(format!("{}.pdf", doc.name));
    shell::status(
        "Finished",
        format!("`{}` profile in {:.2}s → {}", profile.name, started.elapsed().as_secs_f64(), project.display(&pdf)),
    );
    Ok(pdf)
}

fn run(cmd: &mut Command, verbose: bool) -> Result<Output> {
    let result = if verbose {
        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit()).output()
    } else {
        cmd.output()
    };
    result.map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            anyhow!("`latexmk` was not found on PATH\nhelp: install TeX Live or MiKTeX, then run `quire doctor`")
        } else {
            anyhow!(e).context("failed to run latexmk")
        }
    })
}

fn write_if_changed(path: &Path, contents: &str) -> Result<()> {
    if fs::read_to_string(path).ok().as_deref() != Some(contents) {
        fs::write(path, contents).with_context(|| format!("failed to write `{}`", path.display()))?;
    }
    Ok(())
}

/// The file latexmk actually compiles. It sets up the profile, then `\input`s
/// the real main file (latexmk runs in the main file's directory).
///
/// The dependency directories are listed as comments so that any change to the
/// resolved dependencies changes this file, which makes latexmk rebuild.
pub fn driver_source(profile: &Profile, main_file: &str, main_source: &str, search_dirs: &[PathBuf]) -> String {
    let mut out = String::from("% Generated by quire. Do not edit.\n");
    for dir in search_dirs {
        out.push_str(&format!("% search: {}\n", tex_path(dir)));
    }
    out.push_str(&format!("\\def\\QuireProfile{{{}}}\n", profile.name));
    if !profile.class_options.is_empty() {
        match document_class(main_source) {
            Some(class) => out.push_str(&format!(
                "\\PassOptionsToClass{{{}}}{{{class}}}\n",
                profile.class_options.join(",")
            )),
            None => shell::warn(format!(
                "profile `{}` sets class-options, but no \\documentclass was found in `{main_file}`",
                profile.name
            )),
        }
    }
    if let Some(pretex) = &profile.pretex {
        out.push_str(pretex);
        out.push('\n');
    }
    if main_file.contains(' ') {
        out.push_str(&format!("\\input{{\"{main_file}\"}}\n"));
    } else {
        out.push_str(&format!("\\input{{{main_file}}}\n"));
    }
    out
}

/// Strip `%` comments (keeping escaped `\%`).
fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let mut escaped = false;
        for c in line.chars() {
            if c == '%' && !escaped {
                break;
            }
            escaped = c == '\\' && !escaped;
            out.push(c);
        }
        out.push('\n');
    }
    out
}

/// The class named by `\documentclass[...]{class}`.
pub fn document_class(source: &str) -> Option<String> {
    let text = strip_comments(source);
    let start = text.find("\\documentclass")? + "\\documentclass".len();
    let mut rest = text[start..].trim_start();
    if rest.starts_with('[') {
        rest = rest[rest.find(']')? + 1..].trim_start();
    }
    let rest = rest.strip_prefix('{')?;
    let class = rest[..rest.find('}')?].trim();
    (!class.is_empty()).then(|| class.to_string())
}

/// `./chapter.tex:12: Undefined control sequence.` (with -file-line-error)
fn is_file_line_error(line: &str) -> bool {
    line.match_indices(':').any(|(i, _)| {
        let rest = &line[i + 1..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        digits > 0 && rest[digits..].starts_with(": ")
    })
}

/// Errors in a TeX log, each with the `l.NN ...` context lines that follow it.
pub fn log_errors(log: &str) -> Vec<String> {
    let lines: Vec<&str> = log.lines().collect();
    let mut errors = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let noise = line.contains("Emergency stop") || line.contains("==> Fatal error occurred");
        if !noise && (line.starts_with("! ") || is_file_line_error(line)) {
            let mut message = line.trim_start_matches("! ").to_string();
            let mut j = i + 1;
            while j < lines.len() && j <= i + 6 && !lines[j].trim().is_empty() {
                if lines[j].starts_with("l.") {
                    message.push_str("\n  ");
                    message.push_str(lines[j]);
                    if let Some(next) = lines.get(j + 1).filter(|l| !l.trim().is_empty()) {
                        message.push_str("\n  ");
                        message.push_str(next);
                    }
                    break;
                }
                j += 1;
            }
            errors.push(message);
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    errors
}

fn report_warnings(project: &Project, log: &str, log_path: &Path, verbose: bool) {
    if log.contains("LaTeX Warning: Citation") {
        shell::warn("there are undefined citations");
    } else if log.contains("There were undefined references") {
        shell::warn("there are undefined references");
    }
    let count = log.lines().filter(|l| l.contains("Warning:")).count();
    if count > 0 && !verbose {
        shell::warn(format!("{count} TeX warning(s); see {}", project.display(log_path)));
    }
}

/// Warn about distribution packages (e.g. `siunitx = "*"`) that kpsewhich cannot find.
fn check_distribution_deps(resolution: &Resolution) {
    let names: Vec<&str> = resolution
        .deps
        .iter()
        .filter(|d| matches!(d.source, Source::TexLive { .. }))
        .map(|d| d.name.as_str())
        .collect();
    if names.is_empty() {
        return;
    }
    let mut cmd = Command::new("kpsewhich");
    for name in &names {
        cmd.arg(format!("{name}.sty")).arg(format!("{name}.cls"));
    }
    let Ok(out) = cmd.output() else { return };
    let found = String::from_utf8_lossy(&out.stdout).replace('\\', "/");
    for name in names {
        let present = found.lines().any(|l| {
            let file = l.rsplit('/').next().unwrap_or(l);
            file == format!("{name}.sty") || file == format!("{name}.cls")
        });
        if !present {
            shell::warn(format!(
                "`{name}` was not found in your TeX distribution; the build may fail or trigger an on-demand install"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(class_options: &[&str]) -> Profile {
        Profile {
            name: "draft".into(),
            class_options: class_options.iter().map(|s| s.to_string()).collect(),
            pretex: Some("\\def\\Extra{1}".into()),
            synctex: true,
            shell_escape: false,
            latexmk_args: vec![],
        }
    }

    #[test]
    fn finds_document_class() {
        assert_eq!(document_class("\\documentclass{article}"), Some("article".into()));
        assert_eq!(document_class("% \\documentclass{fake}\n\\documentclass[\n 11pt,\n a4paper]{ report }"), Some("report".into()));
        assert_eq!(document_class("50\\% \\documentclass{x}"), Some("x".into()));
        assert_eq!(document_class("no class here"), None);
    }

    #[test]
    fn driver_applies_profile() {
        let d = driver_source(&profile(&["draft", "twoside"]), "main.tex", "\\documentclass{book}", &[PathBuf::from("/deps/x")]);
        assert!(d.contains("% search: /deps/x\n"));
        assert!(d.contains("\\def\\QuireProfile{draft}"));
        assert!(d.contains("\\PassOptionsToClass{draft,twoside}{book}"));
        assert!(d.contains("\\def\\Extra{1}"));
        assert!(d.trim_end().ends_with("\\input{main.tex}"));
        assert!(driver_source(&profile(&[]), "my file.tex", "", &[]).contains("\\input{\"my file.tex\"}"));
    }

    #[test]
    fn extracts_log_errors() {
        let log = "This is pdfTeX\n./main.tex:5: Undefined control sequence.\nl.5 \\foo\n         bar\n\n! LaTeX Error: File `x.sty' not found.\n\nOutput written on main.pdf (1 page).\n";
        let errors = log_errors(log);
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0], "./main.tex:5: Undefined control sequence.\n  l.5 \\foo\n           bar");
        assert_eq!(errors[1], "LaTeX Error: File `x.sty' not found.");
    }
}
