//! End-to-end tests of the `quire` binary. Tests that need git or latexmk skip
//! themselves when the tool is not installed.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn quire(dir: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quire"))
        .current_dir(dir)
        .env("QUIRE_HOME", home)
        .args(args)
        .output()
        .expect("run quire")
}

fn ok(out: &Output) -> String {
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "quire failed:\n{text}");
    text
}

fn has(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "core.autocrlf=false"])
        .args(args)
        .output()
        .unwrap()
        .status;
    assert!(status.success(), "git {args:?} failed");
}

fn file_url(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    if p.starts_with('/') { format!("file://{p}") } else { format!("file:///{p}") }
}

#[test]
fn new_creates_a_valid_project() {
    let tmp = TempDir::new().unwrap();
    ok(&quire(tmp.path(), tmp.path(), &["new", "paper", "--vcs", "none"]));
    let manifest = fs::read_to_string(tmp.path().join("paper/quire.toml")).unwrap();
    assert!(manifest.contains("name = \"paper\""));
    assert!(tmp.path().join("paper/src/main.tex").is_file());

    let again = quire(tmp.path(), tmp.path(), &["new", "paper"]);
    assert!(!again.status.success());
}

#[test]
fn failed_add_leaves_manifest_unchanged() {
    let tmp = TempDir::new().unwrap();
    ok(&quire(tmp.path(), tmp.path(), &["new", "paper", "--vcs", "none"]));
    let dir = tmp.path().join("paper");
    let before = fs::read_to_string(dir.join("quire.toml")).unwrap();
    let out = quire(&dir, tmp.path(), &["add", "missing", "--path", "../does-not-exist"]);
    assert!(!out.status.success());
    assert_eq!(fs::read_to_string(dir.join("quire.toml")).unwrap(), before);
}

#[test]
fn git_dependencies_are_locked_and_updated() {
    if !has("git") {
        eprintln!("skipping: git not installed");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    ok(&quire(tmp.path(), &home, &["new", "macros", "--template", "package", "--vcs", "none"]));
    let pkg = tmp.path().join("macros");
    git(&pkg, &["init", "-q"]);
    git(&pkg, &["add", "-A"]);
    git(&pkg, &["commit", "-qm", "one"]);

    ok(&quire(tmp.path(), &home, &["new", "paper", "--vcs", "none"]));
    let doc = tmp.path().join("paper");
    ok(&quire(&doc, &home, &["add", "macros", "--git", &file_url(&pkg)]));
    let lock = fs::read_to_string(doc.join("quire.lock")).unwrap();
    assert!(lock.contains("name = \"macros\""));
    let first_rev = lock.lines().find(|l| l.starts_with("rev")).unwrap().to_string();

    // A new upstream commit does not move the lock...
    fs::write(pkg.join("tex/extra.sty"), "\\ProvidesPackage{extra}\n").unwrap();
    git(&pkg, &["add", "-A"]);
    git(&pkg, &["commit", "-qm", "two"]);
    ok(&quire(&doc, &home, &["fetch"]));
    assert!(fs::read_to_string(doc.join("quire.lock")).unwrap().contains(&first_rev));

    // ...until `quire update`.
    ok(&quire(&doc, &home, &["update"]));
    assert!(!fs::read_to_string(doc.join("quire.lock")).unwrap().contains(&first_rev));

    // Removing the dependency prunes it from the lockfile.
    ok(&quire(&doc, &home, &["remove", "macros"]));
    assert!(!fs::read_to_string(doc.join("quire.lock")).unwrap().contains("macros"));
}

#[test]
fn builds_with_a_path_dependency() {
    if !has("latexmk") {
        eprintln!("skipping: latexmk not installed");
        return;
    }
    let tmp = TempDir::new().unwrap();
    ok(&quire(tmp.path(), tmp.path(), &["new", "macros", "--template", "package", "--vcs", "none"]));
    ok(&quire(tmp.path(), tmp.path(), &["new", "paper", "--vcs", "none"]));
    let doc = tmp.path().join("paper");
    fs::write(
        doc.join("src/main.tex"),
        "\\documentclass{article}\n\\usepackage{macros}\n\\begin{document}\n\\macroshello\n\\end{document}\n",
    )
    .unwrap();

    let without = quire(&doc, tmp.path(), &["build"]);
    assert!(!without.status.success());
    assert!(String::from_utf8_lossy(&without.stderr).contains("macros.sty"));

    ok(&quire(&doc, tmp.path(), &["add", "macros", "--path", "../macros"]));
    ok(&quire(&doc, tmp.path(), &["build", "--profile", "draft"]));
    assert!(doc.join("target/draft/paper.pdf").is_file());
    let driver = fs::read_to_string(doc.join("target/draft/quire-driver.tex")).unwrap();
    assert!(driver.contains("\\PassOptionsToClass{draft}{article}"));
}
