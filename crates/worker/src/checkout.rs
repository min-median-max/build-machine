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
//!
//! A step that names another repository fetches from that repository's own
//! bundle, made from its local clone, into a mirror of its own, and checks out
//! the step's `ref` into the step's `path`. Only committed history of that
//! repository is checked out.

use crate::stream;
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
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

/// The mirror takes the bundle's refs as a repository on GitHub holds them.
fn update_mirror(history: &Path, mirror: &Path, environment: &[(String, String)]) -> Result<()> {
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
    Ok(())
}

/// What actions/checkout runs: fetch `revision` from the mirror as
/// `fetch-depth` asks and check out its branch or tag, or the commit detached.
fn fetch_and_check_out(
    workspace: &Path,
    mirror: &Path,
    revision: &str,
    reference: Option<&str>,
    fetch_depth: u32,
    fetch_tags: bool,
    environment: &[(String, String)],
) -> Result<()> {
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
    Ok(())
}

pub fn checkout(checkout: &Checkout, environment: &[(String, String)]) -> Result<String> {
    let Checkout { history, mirror, workspace, archive, revision, reference, dirty, fetch_depth, fetch_tags } =
        *checkout;
    if std::fs::read_dir(workspace)?.next().is_some() {
        bail!("GITHUB_WORKSPACE가 비어 있지 않아요: {}", workspace.display());
    }
    update_mirror(history, mirror, environment)?;
    fetch_and_check_out(workspace, mirror, revision, reference, fetch_depth, fetch_tags, environment)?;

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

/// A checkout step that names another repository.
pub struct RepositoryCheckout<'a> {
    /// `owner/name`, as `machine.json` maps it.
    pub repository: &'a str,
    /// The bundle of that repository's history.
    pub history: &'a Path,
    /// A bare repository holding that history, apart from the project's.
    pub mirror: &'a Path,
    /// `GITHUB_WORKSPACE/<path>`, empty or absent.
    pub directory: &'a Path,
    /// The step's `ref`: a branch, a tag or a commit SHA. `None` is the
    /// bundle's `HEAD`.
    pub reference: Option<&'a str>,
    pub fetch_depth: u32,
    pub fetch_tags: bool,
}

/// The commit a `ref` names and the branch or tag it checks out, as
/// actions/checkout reads it: a branch before a tag, a full commit SHA
/// detached, and the bundle's `HEAD` detached when the step names no `ref`.
fn resolve_reference(
    repository: &str,
    mirror: &Path,
    reference: Option<&str>,
    environment: &[(String, String)],
) -> Result<(String, Option<String>)> {
    let unknown = || {
        format!("{repository}에 ref '{}'가 없어요. branch, tag 또는 전체 commit SHA여야 해요.", reference.unwrap_or("HEAD"))
    };
    // Each ref with its commit; an annotated tag's own object is peeled.
    let listed = git(
        mirror,
        &["for-each-ref", "--format=%(refname) %(objectname) %(*objectname)", "refs/heads", "refs/tags", "refs/build-machine"],
        environment,
    )?;
    let commits: BTreeMap<&str, &str> = listed
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let (name, object, peeled) = (fields.next()?, fields.next()?, fields.next().unwrap_or(""));
            Some((name, if peeled.is_empty() { object } else { peeled }))
        })
        .collect();
    let Some(reference) = reference else {
        let head = commits.get("refs/build-machine/HEAD").with_context(unknown)?;
        return Ok(((*head).to_owned(), None));
    };
    let names = if reference.starts_with("refs/heads/") || reference.starts_with("refs/tags/") {
        vec![reference.to_owned()]
    } else {
        vec![format!("refs/heads/{reference}"), format!("refs/tags/{reference}")]
    };
    if let Some((name, revision)) = names.into_iter().find_map(|name| commits.get(name.as_str()).map(|revision| (name.clone(), *revision))) {
        return Ok((revision.to_owned(), Some(name)));
    }
    let sha = matches!(reference.len(), 40 | 64) && reference.chars().all(|value| value.is_ascii_hexdigit());
    if !sha {
        bail!(unknown());
    }
    git(mirror, &["cat-file", "-e", &format!("{reference}^{{commit}}")], environment).with_context(unknown)?;
    Ok((reference.to_ascii_lowercase(), None))
}

pub fn checkout_repository(checkout: &RepositoryCheckout, environment: &[(String, String)]) -> Result<String> {
    let RepositoryCheckout { repository, history, mirror, directory, reference, fetch_depth, fetch_tags } = *checkout;
    if directory.exists() && std::fs::read_dir(directory)?.next().is_some() {
        bail!("{repository}를 checkout할 폴더가 비어 있지 않아요: {}", directory.display());
    }
    std::fs::create_dir_all(directory)?;
    update_mirror(history, mirror, environment)?;
    let (revision, name) = resolve_reference(repository, mirror, reference, environment)?;
    fetch_and_check_out(directory, mirror, &revision, name.as_deref(), fetch_depth, fetch_tags, environment)?;
    let shown = match &name {
        Some(name) => format!("{name} at {revision}"),
        None => format!("{revision}, detached"),
    };
    let depth = if fetch_depth == 0 { "every branch and tag".to_owned() } else { format!("fetch-depth {fetch_depth}") };
    Ok(format!(
        "Checked out {repository} {} as {shown} ({depth}) into {}.",
        reference.unwrap_or("HEAD"),
        directory.display()
    ))
}
