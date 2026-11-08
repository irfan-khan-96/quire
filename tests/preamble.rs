//! Preamble caching against a real TeX installation. Skipped when latexmk or
//! the mylatexformat package is not installed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::SystemTime;

use tempfile::TempDir;

fn quire(dir: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quire"))
        .current_dir(dir)
        .env("QUIRE_HOME", home)
        .env_remove("QUIRE_PREAMBLE_CACHE")
        .args(args)
        .output()
        .expect("run quire")
}

fn ok(out: &Output) -> String {
    let text = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "quire failed:\n{text}");
    text
}

fn available() -> bool {
    let latexmk = Command::new("latexmk").arg("-v").output().is_ok();
    let mylatexformat = Command::new("kpsewhich")
        .arg("mylatexformat.ltx")
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty());
    if !(latexmk && mylatexformat) {
        assert!(
            std::env::var("QUIRE_REQUIRE_TEX").unwrap_or_default().is_empty(),
            "QUIRE_REQUIRE_TEX is set but latexmk or mylatexformat is missing"
        );
        eprintln!("skipping: latexmk or mylatexformat not installed");
    }
    latexmk && mylatexformat
}

const PREAMBLE: &str = "\\documentclass{article}\n\\usepackage{amsmath}\n";

fn project(tmp: &Path, body: &str) -> PathBuf {
    ok(&quire(tmp, tmp, &["new", "paper", "--vcs", "none"]));
    let doc = tmp.join("paper");
    write_main(&doc, PREAMBLE, body);
    doc
}

fn write_main(doc: &Path, preamble: &str, body: &str) {
    fs::write(doc.join("src/main.tex"), format!("{preamble}\\begin{{document}}\n{body}\n\\end{{document}}\n")).unwrap();
}

fn fmt_path(doc: &Path) -> PathBuf {
    doc.join("target/dev/preamble/paper.fmt")
}

fn modified(path: &Path) -> SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

/// The format named on the first line of the TeX log.
fn loaded_format(doc: &Path) -> String {
    let log = fs::read(doc.join("target/dev/paper.log")).unwrap();
    let first = String::from_utf8_lossy(&log).lines().next().unwrap_or_default().to_string();
    first.split("format=").nth(1).unwrap_or_default().split([' ', ')']).next().unwrap_or_default().to_string()
}

#[test]
fn cached_preamble_is_reused_until_the_preamble_changes() {
    if !available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let doc = project(tmp.path(), "First.");

    let first = ok(&quire(&doc, tmp.path(), &["build"]));
    assert!(first.contains("Caching"), "{first}");
    assert!(fmt_path(&doc).is_file());
    assert!(loaded_format(&doc).starts_with("paper"), "log names format {}", loaded_format(&doc));
    let dumped_at = modified(&fmt_path(&doc));

    // Editing the body reuses the format, and TeX really runs again.
    let log = doc.join("target/dev/paper.log");
    let logged_at = modified(&log);
    std::thread::sleep(std::time::Duration::from_millis(50));
    write_main(&doc, PREAMBLE, "Second.");
    let second = ok(&quire(&doc, tmp.path(), &["build"]));
    assert!(!second.contains("Caching"), "{second}");
    assert!(modified(&log) > logged_at, "TeX did not rerun after the body changed");
    assert_eq!(modified(&fmt_path(&doc)), dumped_at);
    assert!(loaded_format(&doc).starts_with("paper"));

    // Editing the preamble rebuilds it.
    write_main(&doc, "\\documentclass{article}\n\\usepackage{amsmath,amssymb}\n", "Third.");
    let third = ok(&quire(&doc, tmp.path(), &["build"]));
    assert!(third.contains("Caching"), "{third}");
}

#[test]
fn errors_in_the_body_are_reported_from_the_cached_build() {
    if !available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let doc = project(tmp.path(), "Fine.");
    ok(&quire(&doc, tmp.path(), &["build"]));
    write_main(&doc, PREAMBLE, "\\nosuchmacro");
    let out = quire(&doc, tmp.path(), &["build"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("main.tex:4: Undefined control sequence"), "{stderr}");
    // No fallback rebuild: the failing run used the format.
    assert!(loaded_format(&doc).starts_with("paper"));
}

#[test]
fn cached_preamble_sees_normal_catcodes() {
    if !available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let doc = project(tmp.path(), "Hi.");
    // mylatexformat leaves `@` a letter; a preamble must not see that, or code
    // that only works with the cache would fail on arXiv.
    write_main(
        &doc,
        "\\documentclass{article}\n\\edef\\atcat{\\the\\catcode`\\@}\n",
        "\\ifnum\\atcat=11 \\errmessage{@ was a letter in the preamble}\\fi Hi.",
    );
    ok(&quire(&doc, tmp.path(), &["build"]));
    assert!(loaded_format(&doc).starts_with("paper"));
}

#[test]
fn uncacheable_preambles_fall_back_and_are_remembered() {
    if !available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let doc = project(tmp.path(), "Hi.");
    // Cut at the first \begin{document}, the dumped preamble has an unclosed brace.
    write_main(&doc, "\\documentclass{article}\n\\newcommand\\weird{\\begin{document}}\n", "Hi.");
    let first = ok(&quire(&doc, tmp.path(), &["build"]));
    assert!(first.contains("could not be cached"), "{first}");
    assert!(doc.join("target/dev/paper.pdf").is_file());

    let second = ok(&quire(&doc, tmp.path(), &["build"]));
    assert!(!second.contains("Caching"), "dump was retried:\n{second}");

    // \endofdump keeps the problem out of the cache.
    write_main(&doc, "\\documentclass{article}\n\\endofdump\n\\newcommand\\weird{\\begin{document}}\n", "Hi.");
    ok(&quire(&doc, tmp.path(), &["build"]));
    assert!(loaded_format(&doc).starts_with("paper"));
}

#[test]
fn cache_can_be_turned_off() {
    if !available() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let doc = project(tmp.path(), "Hi.");
    let manifest = fs::read_to_string(doc.join("quire.toml")).unwrap();
    fs::write(doc.join("quire.toml"), format!("{manifest}\n[build]\npreamble-cache = false\n")).unwrap();
    ok(&quire(&doc, tmp.path(), &["build"]));
    assert!(!fmt_path(&doc).exists());
    assert!(!loaded_format(&doc).starts_with("paper"));
}
