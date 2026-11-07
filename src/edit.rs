//! `quire add` / `quire remove`: edit `quire.toml` in place, keeping comments
//! and formatting, then re-resolve so the lockfile stays in sync.

use std::fs;

use anyhow::{Context, Result, bail};
use toml_edit::{DocumentMut, InlineTable, Item, Table, value};

use crate::manifest::validate_name;
use crate::project::Project;
use crate::resolve::{self, Update};
use crate::shell;

#[derive(Debug, clap::Args)]
pub struct AddArgs {
    /// Dependency name (for a TeX distribution package, its name, e.g. `siunitx`)
    pub name: String,
    /// Use a local directory
    #[arg(long, conflicts_with_all = ["git", "version"])]
    pub path: Option<String>,
    /// Use a git repository
    #[arg(long, conflicts_with = "version")]
    pub git: Option<String>,
    /// Git tag to use
    #[arg(long, requires = "git", conflicts_with_all = ["branch", "rev"])]
    pub tag: Option<String>,
    /// Git branch to track
    #[arg(long, requires = "git", conflicts_with = "rev")]
    pub branch: Option<String>,
    /// Git commit to use
    #[arg(long, requires = "git")]
    pub rev: Option<String>,
    /// Version requirement for a TeX distribution package
    #[arg(long)]
    pub version: Option<String>,
}

pub fn add(project: &Project, args: &AddArgs) -> Result<()> {
    validate_name(&args.name).context("invalid dependency name")?;
    let (item, description) = dependency_item(project, args)?;
    edit_manifest(project, |deps| {
        let replaced = deps.contains_key(&args.name);
        deps.insert(&args.name, item);
        shell::status(if replaced { "Updating" } else { "Adding" }, format!("`{}` ({description})", args.name));
        Ok(())
    })
}

pub fn remove(project: &Project, name: &str) -> Result<()> {
    edit_manifest(project, |deps| {
        if deps.remove(name).is_none() {
            bail!("`{name}` is not a dependency of `{}`", project.manifest.name());
        }
        shell::status("Removing", format!("`{name}`"));
        Ok(())
    })
}

fn dependency_item(project: &Project, args: &AddArgs) -> Result<(Item, String)> {
    if let Some(path) = &args.path {
        // Store paths relative to the project root, as quire.toml interprets them.
        let cwd = std::env::current_dir()?;
        let stored = if cwd == project.root {
            path.clone()
        } else {
            std::path::absolute(cwd.join(path))?.to_string_lossy().replace('\\', "/")
        };
        let mut t = InlineTable::new();
        t.insert("path", stored.as_str().into());
        return Ok((value(t), format!("path {stored}")));
    }
    if let Some(url) = &args.git {
        let mut t = InlineTable::new();
        t.insert("git", url.as_str().into());
        let mut description = format!("git {url}");
        for (key, val) in [("tag", &args.tag), ("branch", &args.branch), ("rev", &args.rev)] {
            if let Some(val) = val {
                t.insert(key, val.as_str().into());
                description.push_str(&format!(", {key} {val}"));
            }
        }
        return Ok((value(t), description));
    }
    let version = args.version.clone().unwrap_or_else(|| "*".to_string());
    Ok((value(version.as_str()), format!("TeX distribution, version {version}")))
}

/// Apply `change` to the `[dependencies]` table, write the manifest, and
/// re-resolve. If resolution fails, the original manifest is restored.
fn edit_manifest(project: &Project, change: impl FnOnce(&mut Table) -> Result<()>) -> Result<()> {
    let path = project.manifest_path();
    let original = fs::read_to_string(&path).with_context(|| format!("failed to read `{}`", path.display()))?;
    let mut doc: DocumentMut = original.parse().with_context(|| format!("failed to parse `{}`", path.display()))?;
    let deps = doc
        .entry("dependencies")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .context("`dependencies` in quire.toml must be a table")?;
    change(deps)?;
    fs::write(&path, doc.to_string()).with_context(|| format!("failed to write `{}`", path.display()))?;

    let check = Project::load(&project.root).and_then(|p| resolve::resolve(&p, &Update::None).map(|_| ()));
    if let Err(e) = check {
        fs::write(&path, original).with_context(|| format!("failed to restore `{}`", path.display()))?;
        return Err(e.context("quire.toml was left unchanged"));
    }
    Ok(())
}
