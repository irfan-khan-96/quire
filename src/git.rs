//! Git dependencies, cached like Cargo's: one bare "db" clone per URL under
//! `~/.quire/git/db`, and one checkout per commit under `~/.quire/git/checkouts`.
//! Uses the `git` CLI so the user's credentials and proxies just work.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};

use crate::manifest::GitRef;
use crate::shell;

fn run_git(dir: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    if let Some(dir) = dir {
        cmd.arg("-C").arg(dir);
    }
    cmd.args(args).env("GIT_TERMINAL_PROMPT", "0");
    let out = cmd.output().map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            anyhow!("`git` was not found on PATH; it is needed for git dependencies")
        } else {
            anyhow!(e).context("failed to run git")
        }
    })?;
    if !out.status.success() {
        bail!("`git {}` failed:\n{}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn short_hash(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Cache directory name for a repository: `<name>-<hash of url>`.
fn ident(name: &str, url: &str) -> String {
    format!("{name}-{}", short_hash(url))
}

pub struct Db {
    pub path: PathBuf,
    /// True if the db was cloned during this call (so it is already up to date).
    pub fresh: bool,
}

/// Make sure a bare clone of `url` exists in the cache.
pub fn ensure_db(home: &Path, name: &str, url: &str, offline: bool) -> Result<Db> {
    let path = home.join("git").join("db").join(ident(name, url));
    if path.join("HEAD").is_file() {
        return Ok(Db { path, fresh: false });
    }
    if offline {
        bail!("`{name}` ({url}) is not in the local cache, and --offline was given");
    }
    fs::create_dir_all(path.parent().unwrap())?;
    shell::status("Cloning", url);
    // Clone next to the final location, then rename, so an interrupted clone
    // never leaves a half-written repository in the cache.
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    if tmp.exists() {
        fs::remove_dir_all(&tmp).with_context(|| format!("failed to clear `{}`", tmp.display()))?;
    }
    let dest = tmp.to_string_lossy().to_string();
    if let Err(e) = run_git(None, &["clone", "--bare", "--quiet", url, &dest]) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e.context(format!("failed to clone `{url}`")));
    }
    if path.exists() {
        fs::remove_dir_all(&path).with_context(|| format!("failed to clear `{}`", path.display()))?;
    }
    fs::rename(&tmp, &path).with_context(|| format!("failed to move clone into `{}`", path.display()))?;
    Ok(Db { path, fresh: true })
}

pub fn fetch(db: &Path, url: &str) -> Result<()> {
    shell::status("Updating", url);
    run_git(
        Some(db),
        &["fetch", "--quiet", "--force", "--prune", url, "+refs/heads/*:refs/heads/*", "+refs/tags/*:refs/tags/*"],
    )
    .with_context(|| format!("failed to fetch `{url}`"))?;
    Ok(())
}

/// Resolve a branch/tag/rev to a full commit hash.
pub fn resolve_ref(db: &Path, reference: &GitRef, url: &str) -> Result<String> {
    let spec = match reference {
        GitRef::DefaultBranch => "HEAD".to_string(),
        GitRef::Branch(b) => format!("refs/heads/{b}"),
        GitRef::Tag(t) => format!("refs/tags/{t}"),
        GitRef::Rev(r) => r.clone(),
    };
    run_git(Some(db), &["rev-parse", "--verify", "--quiet", &format!("{spec}^{{commit}}")])
        .map_err(|_| anyhow!("could not find {reference} in `{url}`"))
}

pub fn has_commit(db: &Path, rev: &str) -> bool {
    run_git(Some(db), &["cat-file", "-e", &format!("{rev}^{{commit}}")]).is_ok()
}

/// Check out `rev` into its own directory (reused across projects) and return it.
pub fn checkout(home: &Path, name: &str, url: &str, db: &Path, rev: &str) -> Result<PathBuf> {
    let short = &rev[..rev.len().min(12)];
    let dir = home.join("git").join("checkouts").join(ident(name, url)).join(short);
    let ok = dir.join(".quire-ok");
    if ok.is_file() {
        return Ok(dir);
    }
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("failed to clear `{}`", dir.display()))?;
    }
    fs::create_dir_all(dir.parent().unwrap())?;
    // A worktree keeps the checkout free of a full `.git` directory.
    run_git(Some(db), &["worktree", "prune"])?;
    let dest = dir.to_string_lossy().to_string();
    run_git(Some(db), &["worktree", "add", "--quiet", "--detach", "--force", &dest, rev])
        .with_context(|| format!("failed to check out {short} of `{url}`"))?;
    fs::write(&ok, "")?;
    Ok(dir)
}
