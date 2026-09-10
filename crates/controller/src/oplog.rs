//! The operation log and the per-platform command logs.
//!
//! The operation log describes the run and says where each platform's command
//! output went. It is written whether the run succeeds or fails: a log that
//! only appears on failure means the record a successful run points at does
//! not exist.
//!
//! Every line a run produces passes through here exactly once, so a front end
//! that wants to display output attaches an observer rather than reaching into
//! the process plumbing.

use anyhow::Result;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Where a line came from, so a reader can tell reports from failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

pub type Observer = Arc<dyn Fn(Stream, &str) + Send + Sync>;

#[derive(Clone)]
pub struct OperationLog {
    path: PathBuf,
    handle: Arc<Mutex<()>>,
    observer: Option<Observer>,
}

impl OperationLog {
    pub fn create(path: PathBuf) -> Result<OperationLog> {
        Self::with_observer(path, None)
    }

    pub fn with_observer(path: PathBuf, observer: Option<Observer>) -> Result<OperationLog> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(OperationLog { path, handle: Arc::new(Mutex::new(())), observer })
    }

    /// A log for a different file that reports to the same observer.
    pub fn sibling(&self, path: PathBuf) -> Result<OperationLog> {
        Self::with_observer(path, self.observer.clone())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn append(&self, text: &str) {
        let _guard = self.handle.lock().unwrap_or_else(|error| error.into_inner());
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = file.write_all(text.as_bytes());
        }
    }

    fn observe(&self, stream: Stream, text: &str) {
        if let Some(observer) = &self.observer {
            observer(stream, text.trim_end_matches(['\r', '\n']));
        }
    }

    /// A timestamped line describing the run itself.
    pub fn note(&self, text: &str) {
        self.append(&format!("{} {text}\n", build_machine_core::now()));
        self.observe(Stream::Stdout, text);
    }

    pub fn command(&self, display: &str) {
        let line = format!("> {display}\n");
        self.append(&line);
        self.observe(Stream::Stdout, &line);
    }

    pub fn exit_code(&self, code: i32) {
        let line = format!("Exit code: {code}\n");
        self.append(&line);
        self.observe(Stream::Stdout, &line);
    }

    /// Worker output, kept in the log exactly as it was produced.
    pub fn raw(&self, stream: Stream, text: &str) {
        self.append(text);
        self.observe(stream, text);
    }

    /// A failure the run must surface even when no worker produced it.
    pub fn failure(&self, text: &str) {
        self.append(&format!("{} ERROR: {text}\n", build_machine_core::now()));
        self.observe(Stream::Stderr, &format!("ERROR: {text}"));
    }
}
