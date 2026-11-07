//! Advisory file locks, like Cargo's: concurrent quire processes must not
//! corrupt the shared package cache or a build directory.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::Path;

use anyhow::{Context, Result};

use crate::shell;

/// Held until dropped; closing the file releases the lock.
#[must_use]
pub struct FileLock {
    _file: File,
}

/// Take an exclusive lock on `path`, waiting (and saying so) if another
/// process holds it.
pub fn acquire(path: &Path, what: &str) -> Result<FileLock> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open lock file `{}`", path.display()))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            shell::status("Blocking", format!("waiting for file lock on {what}"));
            file.lock().with_context(|| format!("failed to lock `{}`", path.display()))?;
        }
        Err(TryLockError::Error(e)) => {
            return Err(e).with_context(|| format!("failed to lock `{}`", path.display()));
        }
    }
    Ok(FileLock { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn second_holder_waits_for_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.lock");
        let first = acquire(&path, "test").unwrap();

        let (tx, rx) = mpsc::channel();
        let p = path.clone();
        let waiter = std::thread::spawn(move || {
            let _second = acquire(&p, "test").unwrap();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "lock was not exclusive");
        drop(first);
        rx.recv_timeout(Duration::from_secs(10)).expect("waiter never got the lock");
        waiter.join().unwrap();
    }
}
