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
    let artifact = messages.iter().find(|m| m["reason"] == "document-artifact").unwrap_or_else(|| {
        let log = fs::read(doc.join("target/dev/paper.log")).unwrap_or_default();
        let verbose = quire(&doc, tmp.path(), &["build", "-v", "--message-format", "json"]);
        let verbose_err = String::from_utf8_lossy(&verbose.stderr).to_string();
        let tail: Vec<&str> = verbose_err.lines().rev().take(80).collect::<Vec<_>>().into_iter().rev().collect();
        let main_now = fs::read_to_string(doc.join("src/main.tex")).unwrap_or_default();
        panic!(
            "no artifact message\nmain.tex now:\n{main_now}\nverbose rerun (exit {:?}):\n{}\nstdout:\n{}\nstderr:\n{}\nlog head: {}\nlog tail:\n{}",
            verbose.status.code(),
            tail.join("\n"),
            String::from_utf8_lossy(&built.stdout),
            String::from_utf8_lossy(&built.stderr),
            String::from_utf8_lossy(&log).lines().next().unwrap_or_default(),
            String::from_utf8_lossy(&log)
                .lines()
                .rev()
                .take(40)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    assert!(artifact["pdf"].as_str().unwrap().ends_with("target/dev/paper.pdf"));
    assert_eq!(messages.last().unwrap()["reason"], "build-finished");
    assert_eq!(messages.last().unwrap()["success"], true);
}

/// Names and contents of the files in a zip.
fn zip_entries(path: &Path) -> std::collections::BTreeMap<String, String> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    (0..archive.len())
        .map(|i| {
            let mut file = archive.by_index(i).unwrap();
            let mut text = String::new();
            std::io::Read::read_to_string(&mut file, &mut text).unwrap();
            (file.name().unwrap().to_string(), text)
        })
        .collect()
}

/// A paper with a chapter, a bibliography, comments and a path dependency.
fn arxiv_project(tmp: &Path) -> std::path::PathBuf {
    ok(&quire(tmp, tmp, &["new", "macros", "--template", "package", "--vcs", "none"]));
    ok(&quire(tmp, tmp, &["new", "paper", "--vcs", "none"]));
    let doc = tmp.join("paper");
    fs::create_dir_all(doc.join("src/chapters")).unwrap();
    fs::write(
        doc.join("src/main.tex"),
        r"\documentclass{article}
\usepackage{macros}
\begin{document}
\macroshello
\input{chapters/intro}
\bibliographystyle{plain}
\bibliography{refs}
\end{document}
",
    )
    .unwrap();
    fs::write(
        doc.join("src/chapters/intro.tex"),
        r"% SECRET reviewer notes
Intro text, 100\% done. % TODO SECRET
See \cite{knuth}.
",
    )
    .unwrap();
    fs::write(doc.join("src/refs.bib"), "@book{knuth, author={D. Knuth}, title={The TeXbook}, year={1984}}\n").unwrap();
    ok(&quire(&doc, tmp, &["add", "macros", "--path", "../macros"]));
    doc
}

#[test]
fn pack_produces_a_self_contained_reproducible_bundle() {
    if !has("latexmk") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let doc = arxiv_project(tmp.path());
    ok(&quire(&doc, tmp.path(), &["pack"]));
    let zip_path = doc.join("target/arxiv/paper-arxiv.zip");
    let first = fs::read(&zip_path).unwrap();

    let entries = zip_entries(&zip_path);
    let names: Vec<&str> = entries.keys().map(String::as_str).collect();
    assert_eq!(names, vec!["chapters/intro.tex", "macros.sty", "main.bbl", "main.tex"]);
    assert!(entries["main.tex"].starts_with("\\pdfoutput=1\n"));
    assert!(entries["main.tex"].contains(r"\def\QuireProfile{release}"));
    assert!(!entries["chapters/intro.tex"].contains("SECRET"));
    assert!(entries["chapters/intro.tex"].contains(r"100\% done. %"));
    assert!(entries["main.bbl"].contains("TeXbook"));

    ok(&quire(&doc, tmp.path(), &["pack"]));
    assert_eq!(fs::read(&zip_path).unwrap(), first, "packing twice gave different bytes");
}

#[test]
fn pack_can_flatten_inputs() {
    if !has("latexmk") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let doc = arxiv_project(tmp.path());
    let manifest = fs::read_to_string(doc.join("quire.toml")).unwrap();
    fs::write(doc.join("quire.toml"), format!("{manifest}\n[target.arxiv]\nflatten = true\n")).unwrap();
    ok(&quire(&doc, tmp.path(), &["pack"]));
    let entries = zip_entries(&doc.join("target/arxiv/paper-arxiv.zip"));
    assert!(!entries.contains_key("chapters/intro.tex"));
    assert!(entries["main.tex"].contains("Intro text"));
    assert!(!entries["main.tex"].contains(r"\input{chapters/intro}"));
}

#[test]
fn pack_rejects_files_arxiv_would_not_have() {
    if !has("latexmk") {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let doc = arxiv_project(tmp.path());
    // A file outside both the project and its dependencies: the local build
    // finds it, arXiv would not.
    let outside = tmp.path().join("outside.tex");
    fs::write(&outside, "Outside text.\n").unwrap();
    let main = fs::read_to_string(doc.join("src/main.tex")).unwrap();
    let outside_path = outside.to_string_lossy().replace('\\', "/");
    fs::write(
        doc.join("src/main.tex"),
        main.replace(r"\macroshello", &format!(r"\macroshello\input{{{outside_path}}}")),
    )
    .unwrap();
    ok(&quire(&doc, tmp.path(), &["build"]));
    let out = quire(&doc, tmp.path(), &["pack"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("outside.tex"), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn pack_refuses_shell_escape() {
    let tmp = TempDir::new().unwrap();
    ok(&quire(tmp.path(), tmp.path(), &["new", "paper", "--vcs", "none"]));
    let doc = tmp.path().join("paper");
    let manifest = fs::read_to_string(doc.join("quire.toml")).unwrap();
    fs::write(doc.join("quire.toml"), format!("{manifest}\n[build]\nshell-escape = true\n")).unwrap();
    let out = quire(&doc, tmp.path(), &["pack"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("shell-escape"));
}
