//! `actions/checkout`: a Git repository at the replayed revision.
//!
//! The controller sends the repository's branches, tags and `HEAD` as a Git
//! bundle beside the source archive. A job's checkout fetches from it what the
//! step's `fetch-depth` asks for, with the commands `actions/checkout` runs —
//! one commit by default, every branch and tag at `fetch-depth: 0` — and checks
//! out the branch or tag of the event, or the commit detached.
//!
//! A replay of the working tree also carries changes that are not committed.
//! They are written over the checkout and staged, on top of the replayed
//! commit: the workflow sees the files that would be committed next, and the
//! history it reads is the repository's own.

use crate::stream;
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::path::Path;

pub struct Checkout<'a> {
    /// The bundle of the repository's history.
    pub history: &'a Path,
    /// A bare repository holding that history, kept between replays.
    pub mirror: &'a Path,
    /// `GITHUB_WORKSPACE`, empty.
    pub workspace: &'a Path,
    /// The source archive: the files of the replay.
    pub archive: &'a Path,
    pub revision: &'a str,
    /// `refs/heads/<branch>` or `refs/tags/<tag>`, or `None` for a commit.
    pub reference: Option<&'a str>,
    /// The archive holds changes that are not committed.
    pub dirty: bool,
    /// `fetch-depth`: 0 is the whole history.
    pub fetch_depth: u32,
    pub fetch_tags: bool,
}

fn git(directory: &Path, arguments: &[&str], environment: &[(String, String)]) -> Result<String> {
    let mut all = vec!["-C".to_owned(), directory.to_string_lossy().into_owned()];
    all.extend(arguments.iter().map(|value| (*value).to_owned()));
    stream::capture("git", &all, environment).with_context(|| format!("git {}", arguments.join(" ")))
}

/// A path as a `file://` URL. A shallow fetch needs Git's transport, which a
/// plain path bypasses.
fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

pub fn checkout(checkout: &Checkout, environment: &[(String, String)]) -> Result<String> {
    let Checkout { history, mirror, workspace, archive, revision, reference, dirty, fetch_depth, fetch_tags } =
        *checkout;
    if std::fs::read_dir(workspace)?.next().is_some() {
        bail!("GITHUB_WORKSPACE가 비어 있지 않아요: {}", workspace.display());
    }

    // The mirror takes the bundle's refs as a repository on GitHub holds them.
    if !mirror.join("HEAD").exists() {
        std::fs::create_dir_all(mirror)?;
        git(mirror, &["init", "--bare", "-q"], environment)?;
    }
    git(mirror, &["config", "uploadpack.allowAnySHA1InWant", "true"], environment)?;
    let bundle = history.to_string_lossy().into_owned();
    git(
        mirror,
        &[
            "fetch",
            "-q",
            "--prune",
            "--force",
            &bundle,
            "+refs/heads/*:refs/heads/*",
            "+refs/tags/*:refs/tags/*",
            "+HEAD:refs/build-machine/HEAD",
        ],
        environment,
    )?;

    // What actions/checkout runs.
    git(workspace, &["init", "-q"], environment)?;
    git(workspace, &["remote", "add", "origin", &file_url(mirror)], environment)?;
    git(workspace, &["config", "--local", "gc.auto", "0"], environment)?;
    let branch = reference.and_then(|name| name.strip_prefix("refs/heads/"));
    let tag = reference.and_then(|name| name.strip_prefix("refs/tags/"));
    let depth = format!("--depth={fetch_depth}");
    let mut fetch = vec!["-c", "protocol.version=2", "fetch"];
    let refspec;
    if fetch_depth == 0 {
        fetch.extend([
            "--prune",
            "--no-recurse-submodules",
            "origin",
            "+refs/heads/*:refs/remotes/origin/*",
            "+refs/tags/*:refs/tags/*",
        ]);
    } else {
        if !fetch_tags {
            fetch.push("--no-tags");
        }
        refspec = match (branch, tag) {
            (Some(branch), _) => format!("+{revision}:refs/remotes/origin/{branch}"),
            (_, Some(tag)) => format!("+{revision}:refs/tags/{tag}"),
            _ => revision.to_owned(),
        };
        fetch.extend(["--prune", "--no-recurse-submodules", &depth, "origin", &refspec]);
    }
    git(workspace, &fetch, environment)?;
    let target;
    let checkout_arguments: Vec<&str> = match (branch, tag) {
        (Some(branch), _) => {
            target = format!("refs/remotes/origin/{branch}");
            vec!["checkout", "-q", "--force", "-B", branch, &target]
        }
        (_, Some(tag)) => {
            target = format!("refs/tags/{tag}");
            vec!["checkout", "-q", "--force", &target]
        }
        _ => vec!["checkout", "-q", "--force", revision],
    };
    git(workspace, &checkout_arguments, environment)?;
    let head = git(workspace, &["rev-parse", "HEAD"], environment)?;
    if head.trim() != revision {
        bail!("checkout이 {revision}가 아니라 {}에 있어요.", head.trim());
    }

    // The files of the replay over the commit: a tracked file the archive
    // does not hold was deleted in the working tree.
    let tracked = git(workspace, &["ls-files", "-z"], environment)?;
    let files: BTreeSet<String> = crate::build::unpack(archive, workspace)?.into_iter().collect();
    for name in tracked.split('\0').filter(|name| !name.is_empty()) {
        if !files.contains(name) {
            std::fs::remove_file(workspace.join(name)).with_context(|| format!("{name}를 지우지 못했어요."))?;
        }
    }
    let shown = match reference {
        Some(reference) => format!("{reference} at {revision}"),
        None => format!("{revision}, detached"),
    };
    let depth = if fetch_depth == 0 { "every branch and tag".to_owned() } else { format!("fetch-depth {fetch_depth}") };
    if dirty {
        git(workspace, &["add", "-A"], environment)?;
        let staged = git(workspace, &["diff", "--cached", "--name-status"], environment)?;
        return Ok(format!(
            "Checked out {shown} ({depth}). Uncommitted changes of the working tree are staged on that commit:\n{}",
            staged.trim_end()
        ));
    }
    let status = git(workspace, &["status", "--porcelain"], environment)?;
    if !status.trim().is_empty() {
        bail!("소스 스냅샷이 {revision}의 파일과 달라요:\n{}", status.trim_end());
    }
    Ok(format!("Checked out {shown} ({depth})."))
}
