use anyhow::{Context, Result};
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

pub struct InboxWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<PathBuf>,
    root: PathBuf,
}

impl InboxWatcher {
    pub fn new(inbox_new_dir: &Path) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let root = inbox_new_dir.to_path_buf();
        let mut watcher = RecommendedWatcher::new(
            move |event: notify::Result<Event>| {
                let Ok(event) = event else {
                    return;
                };
                if !is_arrival_event(&event.kind) {
                    return;
                }
                for path in event.paths.into_iter().filter(|path| is_toml_path(path)) {
                    let _ = tx.send(path);
                }
            },
            Config::default(),
        )
        .with_context(|| format!("Failed to initialize inbox watcher for {}", root.display()))?;

        watcher
            .watch(inbox_new_dir, RecursiveMode::NonRecursive)
            .with_context(|| format!("Failed to watch inbox: {}", inbox_new_dir.display()))?;

        Ok(Self {
            _watcher: watcher,
            rx,
            root,
        })
    }

    pub fn drain_pending(&self) -> Result<Vec<PathBuf>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("Failed to read inbox: {}", self.root.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if !is_toml_path(&path) {
                continue;
            }
            let metadata = entry
                .metadata()
                .with_context(|| format!("Failed to stat message: {}", path.display()))?;
            if !metadata.is_file() {
                continue;
            }
            entries.push((metadata.modified().ok(), path));
        }
        entries.sort_by(|(left_time, left_path), (right_time, right_path)| {
            left_time
                .cmp(right_time)
                .then_with(|| left_path.cmp(right_path))
        });
        Ok(entries.into_iter().map(|(_, path)| path).collect())
    }

    pub fn wait_next(&mut self, timeout: Duration) -> Result<Option<PathBuf>> {
        match self.rx.recv_timeout(timeout) {
            Ok(path) => Ok(Some(path)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("Inbox watcher stopped before an event arrived")
            }
        }
    }
}

fn is_toml_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("toml")
}

fn is_arrival_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Modify(ModifyKind::Name(
                RenameMode::To | RenameMode::Both | RenameMode::Any
            ))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Instant;
    use tempfile::TempDir;

    #[test]
    fn drain_pending_returns_toml_files_in_mtime_order() {
        let temp = TempDir::new().unwrap();
        let inbox = temp.path();
        let second = inbox.join("second.toml");
        let first = inbox.join("first.toml");

        fs::write(&second, "second").unwrap();
        thread::sleep(Duration::from_millis(20));
        fs::write(&first, "first").unwrap();

        let watcher = InboxWatcher::new(inbox).unwrap();
        let pending = watcher.drain_pending().unwrap();

        assert_eq!(pending, vec![second, first]);
    }

    #[test]
    fn drain_pending_skips_non_toml_files() {
        let temp = TempDir::new().unwrap();
        let inbox = temp.path();
        let message = inbox.join("message.toml");
        fs::write(inbox.join("message.tmp"), "tmp").unwrap();
        fs::write(&message, "message").unwrap();

        let watcher = InboxWatcher::new(inbox).unwrap();

        assert_eq!(watcher.drain_pending().unwrap(), vec![message]);
    }

    #[test]
    fn wait_next_returns_file_created_after_watcher_is_armed() {
        let temp = TempDir::new().unwrap();
        let inbox = temp.path();
        let mut watcher = InboxWatcher::new(inbox).unwrap();
        let message = inbox.join("message.toml");

        fs::write(&message, "message").unwrap();

        let observed = watcher.wait_next(Duration::from_secs(2)).unwrap().unwrap();
        assert_eq!(
            observed.canonicalize().unwrap(),
            message.canonicalize().unwrap()
        );
    }

    #[test]
    fn wait_next_returns_none_on_timeout() {
        let temp = TempDir::new().unwrap();
        let mut watcher = InboxWatcher::new(temp.path()).unwrap();
        let started = Instant::now();

        assert_eq!(watcher.wait_next(Duration::from_millis(50)).unwrap(), None);
        assert!(started.elapsed() >= Duration::from_millis(40));
    }

    #[test]
    fn arm_then_list_detects_file_created_before_drain_pending() {
        let temp = TempDir::new().unwrap();
        let inbox = temp.path();
        let watcher = InboxWatcher::new(inbox).unwrap();
        let message = inbox.join("message.toml");

        fs::write(&message, "message").unwrap();

        assert_eq!(watcher.drain_pending().unwrap(), vec![message]);
    }
}
