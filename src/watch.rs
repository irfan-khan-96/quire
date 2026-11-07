//! `quire watch`: rebuild when the project or a path dependency changes.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};

use crate::compile::{self, BuildRequest};
use crate::lock::LOCK_NAME;
use crate::project::Project;
use crate::resolve::{self, Source, Update};
use crate::shell;

pub fn watch(root: &Path, req: &BuildRequest) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let mut debouncer = new_debouncer(Duration::from_millis(300), tx).context("failed to start the file watcher")?;
    let mut watched: BTreeSet<PathBuf> = BTreeSet::new();

    let mut cycle = |watched: &mut BTreeSet<PathBuf>| {
        for dir in build_once(root, req) {
            if watched.contains(&dir) {
                continue;
            }
            match debouncer.watcher().watch(&dir, RecursiveMode::Recursive) {
                Ok(()) => {
                    watched.insert(dir);
                }
                Err(e) => shell::warn(format!("cannot watch `{}`: {e}", dir.display())),
            }
        }
        shell::status("Watching", "for changes (press Ctrl-C to stop)");
    };

    cycle(&mut watched);
    for result in rx {
        match result {
            Ok(events) => {
                if let Some(event) = events.iter().find(|e| is_relevant(&e.path)) {
                    shell::status("Changed", event.path.display());
                    cycle(&mut watched);
                }
            }
            Err(e) => shell::warn(format!("watch error: {e}")),
        }
    }
    Ok(())
}

/// Build once, printing any error, and return the directories to watch
/// (the project plus its path dependencies).
fn build_once(root: &Path, req: &BuildRequest) -> Vec<PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    let result = (|| -> Result<()> {
        let project = Project::load(root)?;
        let resolution = resolve::resolve(&project, &Update::None)?;
        dirs.extend(
            resolution.deps.iter().filter(|d| d.source == Source::Path).filter_map(|d| d.root.clone()),
        );
        compile::build(&project, &resolution, req)?;
        Ok(())
    })();
    if let Err(e) = result {
        shell::error(&e);
    }
    dirs
}

/// Ignore build output, VCS internals and the lockfile quire itself writes.
fn is_relevant(path: &Path) -> bool {
    if path.file_name().is_some_and(|n| n == LOCK_NAME) {
        return false;
    }
    !path.components().any(|c| matches!(c, Component::Normal(n) if n == "target" || n == ".git"))
}
