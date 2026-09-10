//! Source snapshots: what exactly gets built.
//!
//! A snapshot is either the current working tree, including uncommitted edits
//! and non-ignored untracked files, or an immutable Git ref. The archive is
//! written with fixed entry timestamps so identical content always hashes to
//! the same value, which is what makes a build result reusable.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceMode {
    Local,
    Ref,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub revision: String,
    pub dirty: bool,
    pub source_hash: String,
    pub file_count: usize,
    pub source_mode: SourceMode,
    pub project_key: String,
    pub project: String,
    pub archive: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_ref: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stage_counts: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
}

pub fn sha256_bytes(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("파일을 열지 못했어요: {}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

/// A stable directory name for a project path: readable prefix plus a digest of
/// the absolute path, so two projects with the same folder name stay separate.
pub fn project_key(project: &Path) -> String {
    let name: String = project
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = name.trim_matches(['.', '-']);
    let base = if trimmed.is_empty() { "project" } else { trimmed };
    let digest = sha256_bytes(project.to_string_lossy().as_bytes());
    let head: String = base.chars().take(60).collect();
    format!("{head}-{}", &digest[..10])
}

fn git(project: &Path, arguments: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(arguments)
        .output()
        .context("git을 실행하지 못했어요. 명령줄 개발자 도구를 설치해주세요.")?;
    if !output.status.success() {
        bail!("git {:?} 실패: {}", arguments, String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output.stdout)
}

fn git_text(project: &Path, arguments: &[&str]) -> Result<String> {
    Ok(String::from_utf8(git(project, arguments)?)?.trim().to_owned())
}

/// A project is a Git repository root. A monorepo selects a sub-application
/// through a workflow step's `working-directory`, not by registering a
/// subdirectory as its own project.
pub fn repository_root(project: &Path) -> Result<PathBuf> {
    let project = std::fs::canonicalize(project)
        .with_context(|| format!("프로젝트 폴더를 찾지 못했어요: {}", project.display()))?;
    let root = git_text(&project, &["rev-parse", "--show-toplevel"])
        .context("프로젝트 루트가 Git 저장소가 아니에요.")?;
    let root = std::fs::canonicalize(Path::new(&root))?;
    if root != project {
        bail!("프로젝트는 Git 저장소 루트만 등록할 수 있어요. 모노레포 하위 앱은 workflow의 working-directory를 사용하세요.");
    }
    Ok(root)
}

struct Entry {
    name: String,
    mode: u32,
    data: Vec<u8>,
}

fn split_nul(data: &[u8]) -> Vec<&[u8]> {
    data.split(|byte| *byte == 0).filter(|value| !value.is_empty()).collect()
}

fn collect_ref_entries(project: &Path, revision: &str) -> Result<Vec<Entry>> {
    let listing = git(project, &["ls-tree", "-r", "-z", revision])?;
    let mut entries = Vec::new();
    for raw in split_nul(&listing) {
        let text = String::from_utf8_lossy(raw);
        let (metadata, name) = text
            .split_once('\t')
            .with_context(|| format!("git ls-tree 출력을 읽지 못했어요: {text}"))?;
        let mut fields = metadata.splitn(3, ' ');
        let mode = fields.next().unwrap_or_default();
        let kind = fields.next().unwrap_or_default();
        if kind != "blob" || mode.starts_with("120") {
            bail!("고정 ref에 지원하지 않는 Git 항목이 있어요: {name}");
        }
        let data = git(project, &["show", &format!("{revision}:{name}")])?;
        let mode = u32::from_str_radix(&mode[mode.len().saturating_sub(3)..], 8).unwrap_or(0o644);
        entries.push(Entry { name: name.to_owned(), mode, data });
    }
    Ok(entries)
}

fn collect_worktree_entries(project: &Path) -> Result<Vec<Entry>> {
    let listing = git(project, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"])?;
    let mut names: Vec<String> = split_nul(&listing)
        .into_iter()
        .map(|raw| String::from_utf8_lossy(raw).into_owned())
        .collect();
    names.sort();
    names.dedup();
    let mut entries = Vec::new();
    for name in names {
        let path = project.join(&name);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(value) => value,
            // A tracked file deleted in the current working tree.
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink() {
            let resolved = std::fs::canonicalize(&path)
                .with_context(|| format!("Source points outside the project: {name}"))?;
            if !resolved.starts_with(project) {
                bail!("Source points outside the project: {name}");
            }
        }
        if !path.is_file() {
            bail!("Source is not a regular file (including unsupported submodules): {name}");
        }
        let data = std::fs::read(&path)
            .with_context(|| format!("소스를 읽지 못했어요: {name}"))?;
        entries.push(Entry { name, mode: file_mode(&path), data });
    }
    Ok(entries)
}

#[cfg(unix)]
fn file_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map(|value| value.permissions().mode() & 0o777).unwrap_or(0o644)
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> u32 {
    0o644
}

/// Write the archive and return the snapshot facts describing it.
pub fn make_archive(project: &Path, destination: &Path, reference: Option<&str>) -> Result<(String, bool, String, usize, SourceMode)> {
    let project = repository_root(project)?;
    let revision = git_text(&project, &["rev-parse", reference.unwrap_or("HEAD")])?;
    let (mut entries, dirty, mode) = match reference {
        Some(_) => (collect_ref_entries(&project, &revision)?, false, SourceMode::Ref),
        None => {
            let dirty = !git_text(&project, &["status", "--porcelain"])?.is_empty();
            (collect_worktree_entries(&project)?, dirty, SourceMode::Local)
        }
    };
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(destination)
        .with_context(|| format!("아카이브를 만들지 못했어요: {}", destination.display()))?;
    let mut archive = zip::ZipWriter::new(std::io::BufWriter::new(file));
    for entry in &entries {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(entry.mode)
            .last_modified_time(zip::DateTime::default());
        archive.start_file(&entry.name, options)?;
        archive.write_all(&entry.data)?;
    }
    archive.finish()?.flush()?;
    let source_hash = sha256_file(destination)?;
    Ok((revision, dirty, source_hash, entries.len(), mode))
}
