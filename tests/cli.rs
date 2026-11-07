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

/// A package repository with one commit, and a document depending on it.
fn project_with_git_dep(tmp: &Path, home: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    ok(&quire(tmp, home, &["new", "macros", "--template", "package", "--vcs", "none"]));
    let pkg = tmp.join("macros");
    git(&pkg, &["init", "-q"]);
    git(&pkg, &["add", "-A"]);
    git(&pkg, &["commit", "-qm", "one"]);
    ok(&quire(tmp, home, &["new", "paper", "--vcs", "none"]));
    let doc = tmp.join("paper");
    let manifest = fs::read_to_string(doc.join("quire.toml")).unwrap();
    let manifest = manifest.replacen(
        "[dependencies]\n",
        &format!("[dependencies]\nmacros = {{ git = \"{}\" }}\n", file_url(&pkg)),
        1,
    );
    fs::write(doc.join("quire.toml"), manifest).unwrap();
    (pkg, doc)
}

#[test]
fn locked_refuses_to_change_the_lockfile() {
    if !has("git") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let (_, doc) = project_with_git_dep(tmp.path(), &home);

    let out = quire(&doc, &home, &["fetch", "--locked"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not in quire.lock"));
    assert!(!doc.join("quire.lock").exists());

    ok(&quire(&doc, &home, &["fetch"]));
    ok(&quire(&doc, &home, &["fetch", "--locked"]));
    assert!(!quire(&doc, &home, &["update", "--locked"]).status.success());
}

#[test]
fn offline_uses_only_the_cache() {
    if !has("git") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let (_, doc) = project_with_git_dep(tmp.path(), &home);

    let out = quire(&doc, &home, &["fetch", "--offline"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--offline"));

    ok(&quire(&doc, &home, &["fetch"]));
    // Once cached and locked, a frozen resolve needs no network.
    ok(&quire(&doc, &home, &["fetch", "--frozen"]));
}

#[test]
fn json_messages_describe_errors_and_artifacts() {
    if !has("latexmk") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    ok(&quire(tmp.path(), tmp.path(), &["new", "paper", "--vcs", "none"]));
    let doc = tmp.path().join("paper");
    let main = doc.join("src/main.tex");
    fs::write(
        &main,
        r"\documentclass{article}
\begin{document}
\nosuchmacro
\end{document}
",
    )
    .unwrap();

    let parse = |out: &Output| -> Vec<serde_json::Value> {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {l} ({e})")))
            .collect()
    };

    let failed = quire(&doc, tmp.path(), &["build", "--message-format", "json"]);
    assert!(!failed.status.success());
    let messages = parse(&failed);
    let error = messages.iter().find(|m| m["reason"] == "compiler-message").expect("an error message");
    assert_eq!(error["message"]["level"], "error");
    assert_eq!(error["message"]["line"], 3);
    assert!(error["message"]["file"].as_str().unwrap().ends_with("src/main.tex"));
    assert_eq!(messages.last().unwrap()["success"], false);

    fs::write(
        &main,
        r"\documentclass{article}
\begin{document}
Hi
\end{document}
",
    )
    .unwrap();
    let built = quire(&doc, tmp.path(), &["build", "--message-format", "json"]);
    let messages = parse(&built);
    let artifact = messages.iter().find(|m| m["reason"] == "document-artifact").expect("an artifact");
    assert!(artifact["pdf"].as_str().unwrap().ends_with("target/dev/paper.pdf"));
    assert_eq!(messages.last().unwrap()["reason"], "build-finished");
    assert_eq!(messages.last().unwrap()["success"], true);
}
