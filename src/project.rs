//! Locating a project on disk and quire's global home directory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::lock::LOCK_NAME;
use crate::manifest::{MANIFEST_NAME, Manifest};
use crate::shell;

pub struct Project {
    pub root: PathBuf,
    pub manifest: Manifest,
}

impl Project {
    /// Find `quire.toml` in the current directory or any parent, like Cargo.
    pub fn discover() -> Result<Project> {
        let cwd = std::env::current_dir().context("cannot read the current directory")?;
        for dir in cwd.ancestors() {
            if dir.join(MANIFEST_NAME).is_file() {
                return Project::load(dir);
            }
        }
        bail!(
            "could not find `{MANIFEST_NAME}` in `{}` or any parent directory\n\
             help: run `quire new <name>` or `quire init` to create a project",
            cwd.display()
        )
    }

    pub fn load(root: &Path) -> Result<Project> {
        let manifest = Manifest::load(&root.join(MANIFEST_NAME))?;
        for key in manifest.unused.keys() {
            shell::warn(format!("unused manifest key `{key}` in {MANIFEST_NAME}"));
        }
        Ok(Project { root: root.to_path_buf(), manifest })
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join(MANIFEST_NAME)
    }

    pub fn lock_path(&self) -> PathBuf {
        self.root.join(LOCK_NAME)
    }

    pub fn target_dir(&self) -> PathBuf {
        self.root.join("target")
    }

    /// `path` relative to the project root, for display.
    pub fn display(&self, path: &Path) -> String {
        let shown = path.strip_prefix(&self.root).unwrap_or(path);
        shown.to_string_lossy().replace('\\', "/")
    }
}

/// `$QUIRE_HOME`, or `~/.quire`. Holds the git cache.
pub fn quire_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("QUIRE_HOME") {
        return Ok(PathBuf::from(home));
    }
    let home = dirs::home_dir().context("cannot find your home directory; set QUIRE_HOME")?;
    Ok(home.join(".quire"))
}

/// Canonicalize without Windows' `\\?\` prefix, which TeX does not understand.
pub fn normalize(path: &Path) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("`{}` does not exist", path.display()))?;
    let text = canonical.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => Ok(PathBuf::from(rest)),
        _ => Ok(canonical),
    }
}

/// Forward-slash form of a path; TeX and latexmk handle these on every OS.
pub fn tex_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
