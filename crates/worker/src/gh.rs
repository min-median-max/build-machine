//! The `gh` command of a replay.
//!
//! Every replayed job finds this `gh` first on its PATH, so a step never runs
//! the machine's own `gh`, which may be signed in to GitHub. It answers
//! `gh pr view` from the pull request the replay declares (`--pull-request`)
//! and turns `gh pr merge` of that pull request's head commit into a dry run
//! that it records; any other command fails and names itself.

use anyhow::{Context, Result};
use build_machine_core::source::PullRequest;
use std::path::{Path, PathBuf};

const SCRIPT: &str = r#"#!/bin/bash
# build-machine 의 gh 대역이다. 재현은 GitHub 을 바꾸지 않는다.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
declared="$here/pull-request.json"
action="${1:-} ${2:-}"
if [ "$action" != "pr view" ] && [ "$action" != "pr merge" ]; then
  echo "gh $action is not available in a local replay; build-machine answers gh pr view and gh pr merge" >&2
  exit 1
fi
if [ ! -f "$declared" ]; then
  echo "gh $action needs the pull request of the replay; give ci run --pull-request <file>" >&2
  exit 1
fi
shift 2
number="${1:-}"
shift || true
if [ "$number" != "$(jq -r '.number' "$declared")" ]; then
  echo "gh $action: pull request $number is not the declared pull request $(jq -r '.number' "$declared")" >&2
  exit 1
fi
fields="" filter="" match="" method=""
while [ $# -gt 0 ]; do
  case "$1" in
    --repo|-R) shift 2 ;;
    --json) fields="$2"; shift 2 ;;
    --jq|-q) filter="$2"; shift 2 ;;
    --match-head-commit) match="$2"; shift 2 ;;
    --squash|--merge|--rebase) method="${1#--}"; shift ;;
    *) echo "gh $action: $1 is not supported in a local replay" >&2; exit 1 ;;
  esac
done
if [ "$action" = "pr view" ]; then
  for field in ${fields//,/ }; do
    case "$field" in
      files|headRefOid|number) ;;
      *) echo "gh pr view: --json $field is not answered in a local replay; files, headRefOid and number are" >&2; exit 1 ;;
    esac
  done
  view="$(jq '{number: .number, headRefOid: .head_sha, files: [.files[] | {path: .}]}' "$declared")"
  if [ -n "$filter" ]; then
    printf '%s\n' "$view" | jq -r "$filter"
  else
    printf '%s\n' "$view"
  fi
  exit 0
fi
head="$(jq -r '.head_sha' "$declared")"
if [ -z "$method" ]; then
  echo "gh pr merge: give --squash, --merge or --rebase" >&2
  exit 1
fi
if [ -n "$match" ] && [ "$match" != "$head" ]; then
  echo "gh pr merge: the head commit of pull request $number is $head, not $match" >&2
  exit 1
fi
echo "$method $number $head" >> "$here/merges"
echo "dry-run: not merged. Pull request #$number would be $method-merged at $head."
"#;

/// Write the replay's `gh` into `directory` with the declared pull request,
/// and return the folder to put first on the job's PATH.
pub fn install(directory: &Path, pull_request: Option<&PullRequest>) -> Result<PathBuf> {
    let bin = directory.join("build-machine-gh");
    std::fs::create_dir_all(&bin).with_context(|| format!("gh 대역 폴더를 만들지 못했어요: {}", bin.display()))?;
    let script = bin.join("gh");
    std::fs::write(&script, SCRIPT).with_context(|| format!("gh 대역을 쓰지 못했어요: {}", script.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    }
    if let Some(pull_request) = pull_request {
        std::fs::write(bin.join("pull-request.json"), serde_json::to_vec_pretty(pull_request)?)?;
    }
    Ok(bin)
}

/// The merges the replay's `gh` recorded as dry runs, `<method> <number> <sha>`.
pub fn merges(bin: &Path) -> Result<Vec<String>> {
    match std::fs::read_to_string(bin.join("merges")) {
        Ok(text) => Ok(text.lines().map(str::to_owned).collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).context("gh 대역의 merge 기록을 읽지 못했어요."),
    }
}
