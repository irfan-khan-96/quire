//! Preamble caching: compile everything before `\begin{document}` once into a
//! format file and start later builds from it, so pdflatex skips re-reading
//! TikZ, fonts, hyperref and the rest on every run.
//!
//! The format is dumped with the `mylatexformat` package. When a document is
//! later compiled with that format, mylatexformat skips the main file's
//! preamble up to `\begin{document}` (or `\endofdump`, which authors can use to
//! keep packages that cannot be dumped out of the cache). The main file stays
//! the top-level file, so line numbers and SyncTeX are unaffected.
//!
//! The cache key covers the dumped text (profile prelude + preamble), the
//! search-path environment, and the size and modification time of every file
//! the dump read, so editing a package, a dependency or the TeX distribution
//! rebuilds the format. A preamble that cannot be dumped or used is remembered
//! as such (until its key changes) and the build simply runs without the cache.

use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

use crate::compile::comment_start;
use crate::pack::parse_fls;
use crate::project::tex_path;
use crate::shell;

const KEY_FILE: &str = "quire-format.key";
const FAILED_FILE: &str = "quire-format.failed";
const DUMP_FILE: &str = "quire-preamble.tex";

/// A usable cached format.
pub struct Format {
    /// Value for pdflatex's `-fmt=`, relative to the main file's directory.
    pub fmt_arg: String,
    /// True if the format was (re)built during this build.
    pub rebuilt: bool,
    /// Recorder file of the dump: the files the preamble read.
    pub recorder: PathBuf,
    dir: PathBuf,
    key: String,
}

pub struct Inputs<'a> {
    /// Job name, which is also the format's name.
    pub name: &'a str,
    pub root: &'a Path,
    pub main_dir: &'a Path,
    pub out_dir: &'a Path,
    /// TeX that applies the profile (baked into the format).
    pub prelude: &'a str,
    /// The main file's source.
    pub source: &'a str,
    pub env: &'a [(&'static str, OsString)],
}

/// Prepare a cached format for this build, building it if needed. Returns
/// `None` when the cache cannot be used; the build then runs normally.
pub fn prepare(inputs: &Inputs) -> Option<Format> {
    if std::env::var("QUIRE_PREAMBLE_CACHE").is_ok_and(|v| v == "0") {
        return None;
    }
    let split = split_point(inputs.source)?;
    let fmt_arg = fmt_arg(inputs)?;
    let dump_text = format!("{}{}\n\\begin{{document}}\n", inputs.prelude, &inputs.source[..split]);
    let dir = inputs.out_dir.join("preamble");
    let recorder = dir.join(format!("{}.fls", inputs.name));
    let fmt = dir.join(format!("{}.fmt", inputs.name));

    let key = cache_key(&dump_text, inputs.env, &recorder, &dir);
    if read(&dir.join(FAILED_FILE)).as_deref() == Some(key.as_str()) {
        return None;
    }
    if fmt.is_file() && read(&dir.join(KEY_FILE)).as_deref() == Some(key.as_str()) {
        return Some(Format { fmt_arg, rebuilt: false, recorder, dir, key });
    }
    if !has_mylatexformat() {
        return None;
    }

    shell::status("Caching", format!("preamble of `{}`", inputs.name));
    fs::create_dir_all(&dir).ok()?;
    let _ = fs::remove_file(&fmt);
    let dump_file = dir.join(DUMP_FILE);
    fs::write(&dump_file, &dump_text).ok()?;
    let mut cmd = Command::new("pdflatex");
    cmd.current_dir(inputs.main_dir)
        .args(["-ini", "-recorder", "-interaction=nonstopmode", "-halt-on-error"])
        .arg(format!("-jobname={}", inputs.name))
        .arg(format!("-output-directory={}", tex_path(&dir)))
        .arg("&pdflatex")
        .arg("mylatexformat.ltx")
        .arg(tex_path(&dump_file));
    for (var, value) in inputs.env {
        cmd.env(var, value);
    }
    let ok = cmd.output().is_ok_and(|o| o.status.success()) && fmt.is_file();

    // The key includes the files the dump just read.
    let key = cache_key(&dump_text, inputs.env, &recorder, &dir);
    if ok {
        let _ = fs::write(dir.join(KEY_FILE), &key);
        let _ = fs::remove_file(dir.join(FAILED_FILE));
        Some(Format { fmt_arg, rebuilt: true, recorder, dir, key })
    } else {
        let _ = fs::write(dir.join(FAILED_FILE), &key);
        let log = dir.join(format!("{}.log", inputs.name));
        let shown = log.strip_prefix(inputs.root).unwrap_or(&log);
        shell::warn(format!("the preamble could not be cached (see {}); building without the cache", tex_path(shown)));
        None
    }
}

/// Stop using this format until the preamble changes: the document compiled
/// without it but not with it.
pub fn disable(format: &Format) {
    let _ = fs::write(format.dir.join(FAILED_FILE), &format.key);
    shell::warn(
        "this document does not compile from a cached preamble, so the cache is off until the preamble changes \
         (put packages that cannot be cached after \\endofdump to keep the rest cached)",
    );
}

/// Byte offset where the cached part of the preamble ends: the first
/// `\endofdump` or `\begin{document}` outside a comment.
pub fn split_point(source: &str) -> Option<usize> {
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let code = &line[..comment_start(line)];
        let found = ["\\endofdump", "\\begin{document}"].iter().filter_map(|m| code.find(m)).min();
        if let Some(i) = found {
            return Some(offset + i);
        }
        offset += line.len();
    }
    None
}

/// `-fmt=` value relative to the main file's directory. kpathsea only treats a
/// name as a path when it starts with `./` or `../`.
fn fmt_arg(inputs: &Inputs) -> Option<String> {
    let up = inputs.main_dir.strip_prefix(inputs.root).ok()?;
    if up.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    let depth = up.components().count();
    let target = inputs.out_dir.strip_prefix(inputs.root).ok()?.join("preamble").join(inputs.name);
    let prefix = if depth == 0 { "./".to_string() } else { "../".repeat(depth) };
    Some(format!("{prefix}{}", tex_path(&target)))
}

fn cache_key(dump_text: &str, env: &[(&'static str, OsString)], recorder: &Path, dir: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"quire-preamble-v1\n");
    hasher.update(dump_text.as_bytes());
    for (var, value) in env {
        hasher.update(format!("\n{var}={}", value.to_string_lossy()).as_bytes());
    }
    if let Some(fls) = read(recorder) {
        for path in parse_fls(&fls) {
            if path.starts_with(dir) {
                continue;
            }
            let stamp = fs::metadata(&path).ok().map(|m| {
                let modified = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
                format!("{}:{}", m.len(), modified.map(|d| d.as_nanos()).unwrap_or(0))
            });
            hasher.update(format!("\n{}={}", tex_path(&path), stamp.as_deref().unwrap_or("missing")).as_bytes());
        }
    }
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn has_mylatexformat() -> bool {
    Command::new("kpsewhich")
        .arg("mylatexformat.ltx")
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty())
}

fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_end_of_the_cached_preamble() {
        let src =
            "\\documentclass{article}\n% \\begin{document} in a comment\n\\usepackage{x}\n\\begin{document}\nHi\n";
        let split = split_point(src).unwrap();
        assert!(src[split..].starts_with("\\begin{document}\nHi"));
        let with_marker = "\\documentclass{a}\n\\usepackage{x}\n\\endofdump\n\\usepackage{y}\n\\begin{document}\n";
        assert!(with_marker[split_point(with_marker).unwrap()..].starts_with("\\endofdump"));
        assert_eq!(split_point("no document here"), None);
    }

    #[test]
    fn fmt_path_is_explicitly_relative() {
        let env = [];
        let mk = |main_dir: &'static str| Inputs {
            name: "paper",
            root: Path::new("/p"),
            main_dir: Path::new(main_dir),
            out_dir: Path::new("/p/target/dev"),
            prelude: "",
            source: "",
            env: &env,
        };
        assert_eq!(fmt_arg(&mk("/p/src")).unwrap(), "../target/dev/preamble/paper");
        assert_eq!(fmt_arg(&mk("/p")).unwrap(), "./target/dev/preamble/paper");
        assert_eq!(fmt_arg(&mk("/p/a/b")).unwrap(), "../../target/dev/preamble/paper");
    }

    #[test]
    fn key_tracks_text_env_and_recorded_files() {
        let dir = tempfile::tempdir().unwrap();
        let dep = dir.path().join("macros.sty");
        fs::write(&dep, "a").unwrap();
        let recorder = dir.path().join("x.fls");
        fs::write(&recorder, format!("PWD {}\nINPUT {}\n", dir.path().display(), dep.display())).unwrap();
        let cache_dir = dir.path().join("preamble");
        let env = [("TEXINPUTS", OsString::from("a"))];

        let base = cache_key("pre", &env, &recorder, &cache_dir);
        assert_eq!(base, cache_key("pre", &env, &recorder, &cache_dir));
        assert_ne!(base, cache_key("pre2", &env, &recorder, &cache_dir));
        assert_ne!(base, cache_key("pre", &[("TEXINPUTS", OsString::from("b"))], &recorder, &cache_dir));
        fs::write(&dep, "longer").unwrap();
        assert_ne!(base, cache_key("pre", &env, &recorder, &cache_dir));
    }
}
