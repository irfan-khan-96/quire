//! Dependency resolution: walks path and git dependencies (and their own
//! `quire.toml` dependencies), checks for conflicts, and maintains `quire.lock`.

use std::collections::{BTreeMap, VecDeque};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::git;
use crate::lock::{LockedPackage, Lockfile};
use crate::manifest::{DepSpec, MANIFEST_NAME, Manifest};
use crate::project::{Project, normalize, quire_home, tex_path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    TexLive { version: String },
    Path,
    Git { url: String, rev: String },
}

#[derive(Debug, Clone)]
pub struct ResolvedDep {
    pub name: String,
    pub source: Source,
    /// Package root on disk (path and git dependencies only).
    pub root: Option<PathBuf>,
    /// Directory added to TeX's search path (path and git dependencies only).
    pub tex_dir: Option<PathBuf>,
    /// Who asked for this dependency.
    pub required_by: String,
}

pub struct Resolution {
    /// The project's own `[package]` tex directory, if it has one.
    pub own_tex_dir: Option<PathBuf>,
    pub deps: Vec<ResolvedDep>,
}

pub enum Update {
    None,
    All,
    Only(Vec<String>),
}

impl Update {
    fn includes(&self, name: &str) -> bool {
        match self {
            Update::None => false,
            Update::All => true,
            Update::Only(names) => names.iter().any(|n| n == name),
        }
    }
}

struct Pending {
    name: String,
    spec: DepSpec,
    base: PathBuf,
    required_by: String,
    /// Set when the requiring package came from git: path dependencies must
    /// then stay inside that repository.
    repo_root: Option<PathBuf>,
}

pub fn resolve(project: &Project, update: &Update) -> Result<Resolution> {
    let lock_path = project.lock_path();
    let old_lock = Lockfile::load(&lock_path)?;
    let mut new_lock = Lockfile::default();

    let manifest = &project.manifest;
    let own_tex_dir = match &manifest.package {
        Some(pkg) => Some(
            normalize(&project.root.join(&pkg.tex_dir))
                .with_context(|| format!("`package.tex-dir` of `{}`", pkg.name))?,
        ),
        None => None,
    };

    let mut queue = VecDeque::new();
    enqueue(&mut queue, manifest, &project.root, manifest.name(), None)?;

    // name -> source id, to detect the same name coming from two places.
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut deps = Vec::new();
    let mut home: Option<PathBuf> = None;

    while let Some(Pending { name, spec, base, required_by, repo_root }) = queue.pop_front() {
        let id = source_id(&spec, &base);
        if let Some(previous) = seen.get(&name) {
            if *previous == id {
                continue;
            }
            bail!(
                "dependency `{name}` is requested from two different sources:\n  {previous}\n  {id} (required by `{required_by}`)"
            );
        }
        seen.insert(name.clone(), id.clone());

        let (source, root, dep_repo_root) = match spec {
            DepSpec::TexLive { version } => {
                deps.push(ResolvedDep { name, source: Source::TexLive { version }, root: None, tex_dir: None, required_by });
                continue;
            }
            DepSpec::Path { path } => {
                let root = normalize(&base.join(&path));
                if let Some(repo) = &repo_root
                    && !root.as_ref().is_ok_and(|r| r.starts_with(repo)) {
                        bail!(
                            "`{required_by}` comes from git, so its path dependency `{name}` (`{}`) must be inside the same repository\n\
                             help: make `{name}` a git dependency in `{required_by}`'s quire.toml",
                            tex_path(&path)
                        );
                    }
                let root = root.with_context(|| format!("path dependency `{name}` (required by `{required_by}`)"))?;
                (Source::Path, root, repo_root.clone())
            }
            DepSpec::Git { url, reference } => {
                if home.is_none() {
                    home = Some(quire_home()?);
                }
                let home = home.as_deref().unwrap();
                let source = format!("git+{url}{}", reference.query());
                let locked = match update.includes(&name) {
                    true => None,
                    false => old_lock.find(&name, &source).map(|p| p.rev.clone()),
                };
                let db = git::ensure_db(home, &name, &url)?;
                let rev = match locked {
                    Some(rev) => {
                        if !git::has_commit(&db.path, &rev) {
                            git::fetch(&db.path, &url)?;
                            if !git::has_commit(&db.path, &rev) {
                                bail!(
                                    "locked revision {rev} of `{name}` no longer exists in `{url}`\n\
                                     help: run `quire update {name}` to pick a new revision"
                                );
                            }
                        }
                        rev
                    }
                    None => {
                        if !db.fresh {
                            git::fetch(&db.path, &url)?;
                        }
                        git::resolve_ref(&db.path, &reference, &url)
                            .with_context(|| format!("git dependency `{name}`"))?
                    }
                };
                let root = git::checkout(home, &name, &url, &db.path, &rev)?;
                new_lock.packages.push(LockedPackage { name: name.clone(), source, rev: rev.clone() });
                let repo = root.clone();
                (Source::Git { url, rev }, root, Some(repo))
            }
        };

        let tex_dir = visit_package(&name, &root, &mut queue, dep_repo_root)?;
        deps.push(ResolvedDep { name, source, root: Some(root), tex_dir: Some(tex_dir), required_by });
    }

    if !new_lock.packages.is_empty() || lock_path.exists() {
        new_lock.save(&lock_path)?;
    }
    Ok(Resolution { own_tex_dir, deps })
}

fn enqueue(
    queue: &mut VecDeque<Pending>,
    manifest: &Manifest,
    base: &Path,
    required_by: &str,
    repo_root: Option<PathBuf>,
) -> Result<()> {
    for (name, raw) in &manifest.dependencies {
        queue.push_back(Pending {
            name: name.clone(),
            spec: raw.to_spec(name)?,
            base: base.to_path_buf(),
            required_by: required_by.to_string(),
            repo_root: repo_root.clone(),
        });
    }
    Ok(())
}

/// Read a dependency's own `quire.toml` (if any), queue its dependencies and
/// return the directory to expose to TeX.
fn visit_package(name: &str, root: &Path, queue: &mut VecDeque<Pending>, repo_root: Option<PathBuf>) -> Result<PathBuf> {
    let manifest_path = root.join(MANIFEST_NAME);
    if !manifest_path.is_file() {
        // A plain directory or repository of .sty/.cls files.
        return Ok(root.to_path_buf());
    }
    let manifest = Manifest::load(&manifest_path)?;
    let Some(pkg) = &manifest.package else {
        bail!(
            "dependency `{name}` at `{}` is a document, not a package (its quire.toml has no [package] section)",
            root.display()
        );
    };
    if pkg.name != name {
        bail!(
            "dependency `{name}` at `{}` is actually package `{}`\nhelp: rename the dependency to `{}` in quire.toml",
            root.display(),
            pkg.name,
            pkg.name
        );
    }
    enqueue(queue, &manifest, root, name, repo_root)?;
    normalize(&root.join(&pkg.tex_dir)).with_context(|| format!("`tex-dir` of package `{name}`"))
}

fn source_id(spec: &DepSpec, base: &Path) -> String {
    match spec {
        DepSpec::TexLive { .. } => "the TeX distribution".to_string(),
        DepSpec::Path { path } => {
            let joined = base.join(path);
            let shown = normalize(&joined).unwrap_or(joined);
            format!("path+{}", tex_path(&shown))
        }
        DepSpec::Git { url, reference } => format!("git+{url}{}", reference.query()),
    }
}

impl Resolution {
    /// Directories TeX should search, in priority order.
    pub fn search_dirs(&self) -> Vec<PathBuf> {
        self.own_tex_dir
            .iter()
            .chain(self.deps.iter().filter_map(|d| d.tex_dir.as_ref()))
            .cloned()
            .collect()
    }

    /// Environment variables pointing TeX, BibTeX and Biber at the dependencies.
    pub fn search_env(&self) -> Vec<(&'static str, OsString)> {
        let dirs = self.search_dirs();
        if dirs.is_empty() {
            return Vec::new();
        }
        ["TEXINPUTS", "BIBINPUTS", "BSTINPUTS"]
            .into_iter()
            .map(|var| {
                let existing = env::var(var).ok();
                (var, OsString::from(search_path_value(&dirs, existing.as_deref())))
            })
            .collect()
    }
}

/// Build a kpathsea search path: `.`, each directory searched recursively
/// (`dir//`), then the user's existing value or (via the trailing separator)
/// the distribution's default path.
pub fn search_path_value(dirs: &[PathBuf], existing: Option<&str>) -> String {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut parts = vec![".".to_string()];
    parts.extend(dirs.iter().map(|d| format!("{}//", tex_path(d).trim_end_matches('/'))));
    let ours = parts.join(sep);
    match existing {
        Some(existing) if !existing.is_empty() => format!("{ours}{sep}{existing}"),
        _ => format!("{ours}{sep}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_path_appends_default() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let dirs = vec![PathBuf::from("/a/b"), PathBuf::from("/c/")];
        assert_eq!(search_path_value(&dirs, None), format!(".{sep}/a/b//{sep}/c//{sep}"));
        assert_eq!(search_path_value(&dirs, Some("/x")), format!(".{sep}/a/b//{sep}/c//{sep}/x"));
    }
}
