//! `softprops/action-gh-release`.
//!
//! The replay publishes nothing. It resolves the release the action would
//! make — the tag, the files its `files` patterns match and the body file —
//! and records each asset with its size and SHA-256, so a rehearsal can be
//! compared with the release GitHub makes.

use anyhow::{bail, Context, Result};
use build_machine_core::report::Artifact;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The release the action would have made.
#[derive(Debug)]
pub struct Release {
    pub tag: String,
    /// Assets by file name, as the action uploads them.
    pub assets: Vec<Artifact>,
    /// Patterns that matched no file. The action warns about each and
    /// publishes the rest.
    pub unmatched: Vec<String>,
    pub body: Option<Artifact>,
}

/// Resolve the release of a step with the inputs `with` in `workspace` for
/// the replayed `github_ref`.
pub fn publish(workspace: &Path, with: &BTreeMap<String, String>, github_ref: &str) -> Result<Release> {
    let tag = match with.get("tag_name") {
        Some(tag) => tag.clone(),
        None => match github_ref.strip_prefix("refs/tags/") {
            Some(tag) => tag.to_owned(),
            None => bail!("action-gh-release는 tag가 필요해요: {github_ref}는 tag가 아니고 tag_name도 없어요."),
        },
    };
    let mut assets = Vec::new();
    let mut unmatched = Vec::new();
    for pattern in with.get("files").map(String::as_str).unwrap_or("").lines().map(str::trim).filter(|line| !line.is_empty()) {
        let found = matches(workspace, pattern)?;
        if found.is_empty() {
            unmatched.push(pattern.to_owned());
        }
        for path in found {
            let name = path.file_name().context("asset에 파일 이름이 없어요.")?.to_string_lossy().into_owned();
            if assets.iter().any(|asset: &Artifact| asset.path == name) {
                bail!("action-gh-release asset 이름 {name}이 두 파일에 쓰였어요.");
            }
            assets.push(Artifact {
                path: name,
                sha256: build_machine_core::source::sha256_file(&path)?,
                size: std::fs::metadata(&path)?.len(),
            });
        }
    }
    assets.sort_by(|left, right| left.path.cmp(&right.path));
    let body = match with.get("body_path") {
        Some(relative) => {
            let path = crate::build::contained_in(workspace, &workspace.join(relative))
                .filter(|path| path.is_file())
                .with_context(|| format!("action-gh-release의 body_path가 workspace 안의 파일이 아니에요: {relative}"))?;
            Some(Artifact {
                path: relative.clone(),
                sha256: build_machine_core::source::sha256_file(&path)?,
                size: std::fs::metadata(&path)?.len(),
            })
        }
        None => None,
    };
    Ok(Release { tag, assets, unmatched, body })
}

/// The files `pattern` matches under `workspace`, sorted. `*` and `?` match
/// within one path segment and not a leading dot; other glob forms are
/// refused rather than read differently.
fn matches(workspace: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let segments: Vec<&str> = pattern.split('/').collect();
    let unsupported = pattern.starts_with('/')
        || pattern.starts_with('!')
        || pattern.contains(['{', '}', '[', ']', '\\'])
        || segments.iter().any(|segment| segment.is_empty() || *segment == ".." || *segment == "**");
    if unsupported {
        bail!("action-gh-release files 패턴 {pattern}은 재현이 지원하지 않는 형식이에요. workspace 안의 상대 경로에 '*'와 '?'만 쓸 수 있어요.");
    }
    let mut current = vec![workspace.to_path_buf()];
    for segment in segments {
        let mut next = Vec::new();
        for directory in &current {
            if !segment.contains(['*', '?']) {
                let path = directory.join(segment);
                if path.exists() {
                    next.push(path);
                }
                continue;
            }
            if !directory.is_dir() {
                continue;
            }
            let mut names: Vec<String> = std::fs::read_dir(directory)
                .with_context(|| format!("{}를 읽지 못했어요.", directory.display()))?
                .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
                .collect::<std::io::Result<_>>()?;
            names.sort();
            for name in names {
                if (!name.starts_with('.') || segment.starts_with('.')) && wildcard(segment.as_bytes(), name.as_bytes()) {
                    next.push(directory.join(name));
                }
            }
        }
        current = next;
    }
    Ok(current.into_iter().filter(|path| path.is_file()).collect())
}

/// Whether `name` matches `pattern`, where `*` is any run of bytes and `?` one.
fn wildcard(pattern: &[u8], name: &[u8]) -> bool {
    match (pattern.first(), name.first()) {
        (None, None) => true,
        (Some(b'*'), _) => wildcard(&pattern[1..], name) || (!name.is_empty() && wildcard(pattern, &name[1..])),
        (Some(b'?'), Some(_)) => wildcard(&pattern[1..], &name[1..]),
        (Some(expected), Some(actual)) if expected == actual => wildcard(&pattern[1..], &name[1..]),
        _ => false,
    }
}
