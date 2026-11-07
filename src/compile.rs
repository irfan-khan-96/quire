//! Compiling a document: writes a small driver file that applies the profile,
//! runs latexmk with the dependency search path, and reports the TeX log.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};

use crate::diagnostics::{self, Reporter};
use crate::filelock;
use crate::manifest::{Engine, Profile};
use crate::preamble;
use crate::project::{Project, tex_path};
use crate::resolve::{Resolution, Source};
use crate::shell;

pub struct BuildRequest {
    pub profile: String,
    pub verbose: bool,
    pub reporter: Reporter,
}

/// Build the project's document.
pub fn build(project: &Project, resolution: &Resolution, req: &BuildRequest) -> Result<BuildOutput> {
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
    // Two builds of the same profile would overwrite each other's aux files.
    let _lock = filelock::acquire(&out_dir.join(".quire-lock"), "build directory")?;

    check_distribution_deps(resolution);

    let source = fs::read_to_string(&main).with_context(|| format!("failed to read `{}`", main.display()))?;
    let driver = out_dir.join("quire-driver.tex");
    let search_env = resolution.search_env(main_dir);
    let fingerprint: Vec<String> =
        search_env.iter().map(|(var, value)| format!("{var}={}", value.to_string_lossy())).collect();
    write_if_changed(&driver, &driver_source(&profile, &main_file, &source, &fingerprint))?;

    shell::status("Compiling", format!("{} ({}, profile `{}`)", doc.name, doc.engine, profile.name));
    let started = Instant::now();

    let mut format = if doc.engine == Engine::Pdflatex && profile.preamble_cache {
        preamble::prepare(&preamble::Inputs {
            name: &doc.name,
            root: &project.root,
            main_dir,
            out_dir: &out_dir,
            prelude: &profile_prelude(&profile, &main_file, &source),
            source: &source,
            env: &search_env,
        })
    } else {
        None
    };

    // latexmk tracks neither the format file nor which file is top-level, so
    // force a full run whenever either changes. Also force one after a failed
    // build: some latexmk versions keep reporting the old failure without
    // recompiling, even after the source was fixed.
    let mode_file = out_dir.join("quire-mode");
    let failed_marker = out_dir.join("quire-failed");
    let mode = if format.is_some() { "format" } else { "driver" };
    let mode_changed = fs::read_to_string(&mode_file).ok().as_deref() != Some(mode);
    let force = mode_changed || failed_marker.exists() || format.as_ref().is_some_and(|f| f.rebuilt);
    let _ = fs::write(&mode_file, mode);

    let latexmk = |format: Option<&preamble::Format>, force: bool| {
        let mut cmd = Command::new("latexmk");
        cmd.current_dir(main_dir)
            .arg(doc.engine.latexmk_flag())
            .args(["-interaction=nonstopmode", "-halt-on-error", "-file-line-error"])
            .arg(format!("-outdir={}", tex_path(&out_dir)))
            .arg(format!("-jobname={}", doc.name));
        if force {
            cmd.arg("-g");
        }
        if profile.synctex {
            cmd.arg("-synctex=1");
        }
        if profile.shell_escape {
            cmd.arg("-shell-escape");
        }
        cmd.args(&project.manifest.build.latexmk_args).args(&profile.latexmk_args);
        match format {
            // The profile prelude is inside the format, and the main file is
            // compiled directly so line numbers and SyncTeX point at it.
            Some(f) => cmd.arg(format!("-pdflatex=pdflatex -fmt={} %O %S", f.fmt_arg)).arg(&main_file),
            None => cmd.arg(tex_path(&driver)),
        };
        for (var, value) in &search_env {
            cmd.env(var, value);
        }
        cmd
    };
    let attempt = |format: Option<&preamble::Format>, force: bool| -> Result<Output> {
        let output = run(&mut latexmk(format, force), req)?;
        if !output.status.success() && !force && is_stale_failure(&output) {
            // latexmk remembers that BibTeX/Biber failed and refuses to retry
            // until their inputs change, even when the cause (e.g. the search
            // path) was fixed. Force one full rebuild.
            return run(&mut latexmk(format, true), req);
        }
        Ok(output)
    };

    let mut output = attempt(format.as_ref(), force)?;
    let log_path = out_dir.join(format!("{}.log", doc.name));
    if !output.status.success()
        && !error_is_in_body(&log_path, main_dir, &main, &source)
        && let Some(f) = format.take()
    {
        // Some preambles cannot run from a format. If the document builds the
        // normal way, turn the cache off for it; otherwise this is a real error,
        // reported from the normal build.
        output = attempt(None, true)?;
        let _ = fs::write(&mode_file, "driver");
        if output.status.success() {
            preamble::disable(&f);
        }
    }

    let log = fs::read(&log_path).ok().map(|b| String::from_utf8_lossy(&b).into_owned());
    let reporter = req.reporter;

    if !output.status.success() {
        let mut errors = log.as_deref().map(diagnostics::parse_errors).unwrap_or_default();
        errors.iter_mut().for_each(|e| e.resolve_file(main_dir));
        if errors.is_empty() && !req.verbose && !reporter.is_json() {
            let text =
                format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            let lines: Vec<&str> = text.lines().collect();
            for line in &lines[lines.len().saturating_sub(25)..] {
                eprintln!("  {line}");
            }
        }
        let shown = if reporter.is_json() { errors.len() } else { 5 };
        for error in errors.iter().take(shown) {
            reporter.diagnostic(&doc.name, &profile.name, error);
        }
        if errors.len() > shown {
            shell::note(format!("{} more errors not shown", errors.len() - shown));
        }
        if log.is_some() {
            shell::note(format!("full log: {}", project.display(&log_path)));
        }
        let _ = fs::write(&failed_marker, "");
        bail!("could not compile `{}`", doc.name);
    }
    let _ = fs::remove_file(&failed_marker);

    if let Some(log) = &log {
        let warnings = diagnostics::parse_warnings(log);
        if reporter.is_json() {
            for warning in &warnings {
                reporter.diagnostic(&doc.name, &profile.name, warning);
            }
        } else {
            summarize_warnings(project, log, warnings.len(), &log_path, req.verbose);
        }
    }

    let pdf = out_dir.join(format!("{}.pdf", doc.name));
    reporter.artifact(&doc.name, &profile.name, &pdf, &log_path);
    shell::status(
        "Finished",
        format!("`{}` profile in {:.2}s → {}", profile.name, started.elapsed().as_secs_f64(), project.display(&pdf)),
    );
    // With a cached preamble, the files the preamble read are only in the
    // dump's recorder file.
    let mut recorder_files = vec![out_dir.join(format!("{}.fls", doc.name))];
    recorder_files.extend(format.map(|f| f.recorder));
    Ok(BuildOutput { pdf, recorder_files })
}

pub struct BuildOutput {
    pub pdf: PathBuf,
    /// TeX recorder (`.fls`) files listing every file the build read.
    pub recorder_files: Vec<PathBuf>,
}

fn run(cmd: &mut Command, req: &BuildRequest) -> Result<Output> {
    if req.verbose {
        // In JSON mode stdout carries only JSON, so latexmk's output goes to stderr.
        let stdout = if req.reporter.is_json() { Stdio::from(io::stderr()) } else { Stdio::inherit() };
        cmd.stdout(stdout).stderr(Stdio::inherit());
    }
    cmd.output().map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            anyhow!("`latexmk` was not found on PATH\nhelp: install TeX Live or MiKTeX, then run `quire doctor`")
        } else {
            anyhow!(e).context("failed to run latexmk")
        }
    })
}

/// Whether the first error in the log is clearly the document's own: after
/// the cached preamble in the main file, or in another file. Such an error
/// cannot be caused by the preamble cache, so there is no point retrying
/// without it.
fn error_is_in_body(log_path: &Path, main_dir: &Path, main: &Path, source: &str) -> bool {
    let Ok(log) = fs::read(log_path) else { return false };
    let Some(mut error) = diagnostics::parse_errors(&String::from_utf8_lossy(&log)).into_iter().next() else {
        return false;
    };
    error.resolve_file(main_dir);
    let (Some(file), Some(line)) = (error.file, error.line) else { return false };
    if file != tex_path(main) {
        return true;
    }
    let preamble_lines = preamble::split_point(source).map(|i| source[..i].lines().count()).unwrap_or(0);
    line as usize > preamble_lines
}

/// latexmk failed only because a tool failed in an earlier run.
fn is_stale_failure(output: &Output) -> bool {
    String::from_utf8_lossy(&output.stdout).contains("gave an error in previous invocation")
        || String::from_utf8_lossy(&output.stderr).contains("gave an error in previous invocation")
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
/// The search-path environment is written as comments (`fingerprint`), so any
/// change to the resolved dependencies changes this file. latexmk does not
/// track environment variables, so this is what makes it rebuild (and retry a
/// failed BibTeX run).
pub fn driver_source(profile: &Profile, main_file: &str, main_source: &str, fingerprint: &[String]) -> String {
    let mut out = String::from("% Generated by quire. Do not edit.\n");
    for line in fingerprint {
        out.push_str(&format!("% {line}\n"));
    }
    out.push_str(&profile_prelude(profile, main_file, main_source));
    if main_file.contains(' ') {
        out.push_str(&format!("\\input{{\"{main_file}\"}}\n"));
    } else {
        out.push_str(&format!("\\input{{{main_file}}}\n"));
    }
    out
}

/// TeX that applies a profile: defines `\QuireProfile`, passes class options
/// and runs `pretex`. It goes before the main file: in the driver, or at the
/// top of a packed main file.
pub fn profile_prelude(profile: &Profile, main_file: &str, main_source: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("\\def\\QuireProfile{{{}}}\n", profile.name));
    if !profile.class_options.is_empty() {
        match document_class(main_source) {
            Some(class) => {
                out.push_str(&format!("\\PassOptionsToClass{{{}}}{{{class}}}\n", profile.class_options.join(",")))
            }
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
    out
}

/// Strip `%` comments (keeping escaped `\%`). Only for scanning, not for
/// rewriting files: see `pack::strip_tex_comments` for that.
pub fn strip_comments(source: &str) -> String {
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

/// Index where code ends and a `%` comment starts (or the line length).
pub fn comment_start(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'%' => return i,
            _ => i += 1,
        }
    }
    line.len()
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

fn summarize_warnings(project: &Project, log: &str, count: usize, log_path: &Path, verbose: bool) {
    if log.contains("LaTeX Warning: Citation") {
        shell::warn("there are undefined citations");
    } else if log.contains("There were undefined references") {
        shell::warn("there are undefined references");
    }
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
            preamble_cache: true,
            latexmk_args: vec![],
        }
    }

    #[test]
    fn finds_document_class() {
        assert_eq!(document_class("\\documentclass{article}"), Some("article".into()));
        assert_eq!(
            document_class("% \\documentclass{fake}\n\\documentclass[\n 11pt,\n a4paper]{ report }"),
            Some("report".into())
        );
        assert_eq!(document_class("50\\% \\documentclass{x}"), Some("x".into()));
        assert_eq!(document_class("no class here"), None);
    }

    #[test]
    fn driver_applies_profile() {
        let d = driver_source(
            &profile(&["draft", "twoside"]),
            "main.tex",
            "\\documentclass{book}",
            &["TEXINPUTS=/deps/x//".to_string()],
        );
        assert!(d.contains("% TEXINPUTS=/deps/x//\n"));
        assert!(d.contains("\\def\\QuireProfile{draft}"));
        assert!(d.contains("\\PassOptionsToClass{draft,twoside}{book}"));
        assert!(d.contains("\\def\\Extra{1}"));
        assert!(d.trim_end().ends_with("\\input{main.tex}"));
        assert!(driver_source(&profile(&[]), "my file.tex", "", &[]).contains("\\input{\"my file.tex\"}"));
    }
}
