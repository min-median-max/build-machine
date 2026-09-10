//! The repository workflow read as this machine's build contract.

pub mod adapter;
pub mod parse;
pub mod stage;

pub use adapter::{Adapter, STAGE_ORDER};
pub use parse::{Job, Step, Workflow};
pub use stage::{check_gates, stage_counts, stage_of, stages};

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Read and validate a workflow file.
pub fn load(path: &Path, event: &str, reference: Option<&str>) -> Result<Workflow> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("워크플로 파일을 찾을 수 없어요: {}", path.display()))?;
    let workflow = parse::parse(&path.to_string_lossy(), &source, event, reference)?;
    check_gates(&workflow)?;
    Ok(workflow)
}

/// Read and validate workflow text that was extracted from an immutable ref.
pub fn load_text(path: &str, source: &str, event: &str, reference: Option<&str>) -> Result<Workflow> {
    let workflow = parse::parse(path, source, event, reference)?;
    check_gates(&workflow)?;
    Ok(workflow)
}

/// Locate the workflow to replay. A repository with more than one must say
/// which, rather than the machine picking for it.
pub fn discover(root: &Path, requested: Option<&str>, event: &str, reference: Option<&str>) -> Result<Workflow> {
    if let Some(requested) = requested {
        let path = PathBuf::from(requested);
        let path = if path.is_absolute() { path } else { root.join(path) };
        return load(&path, event, reference);
    }
    let directory = root.join(".github").join("workflows");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    matches!(path.extension().and_then(|value| value.to_str()), Some("yml") | Some("yaml"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    match files.len() {
        0 => bail!(".github/workflows에 워크플로가 없어요."),
        1 => load(&files[0], event, reference),
        _ => bail!("워크플로가 여러 개예요. --workflow로 하나를 선택해야 해요."),
    }
}
