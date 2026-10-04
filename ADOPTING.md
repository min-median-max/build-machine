# Making a project buildable on three operating systems

[한국어](ADOPTING.ko.md) · [what the machine supports](PROJECTS.md)

This is what to change in your own repository so this machine can build it on
Windows, Ubuntu and macOS, and rehearse its release before you tag one.

Nothing here is specific to this machine. Every step is something a project
that claims to ship on three operating systems should be able to answer anyway:
which commit was built, whether the tests ran, whether the application starts.
The machine only refuses to guess when the answer is missing.

Work through the checklist, then run `build-machine ci validate` — it tells you
exactly which item is not satisfied yet.

---

## 1. The repository is the unit

- **Register the Git repository root**, not a subdirectory. A monorepo selects
  its sub-application through a workflow step's `working-directory`.
- **Commit your lockfiles.** `pnpm-lock.yaml` or `package-lock.json`, and
  `src-tauri/Cargo.lock` for a Tauri project. A build here is `--frozen-lockfile`
  and `--locked`; a lockfile that drifts fails rather than resolving something
  new.
- **No symlinks out of the project, and no submodules.** The snapshot copies
  tracked files and non-ignored untracked files, including uncommitted edits.
  Anything reaching outside it is refused instead of being silently followed.
- **Keep machine-local secrets out of tracked files.** Ignored files are not
  copied, which is also why a build must not depend on one.

## 2. The build must run unattended

- No interactive prompts, no development server, no absolute paths belonging to
  one developer's machine.
- Use the framework's production build. For Tauri that is `tauri build`;
  the machine passes `--ci --no-sign --locked` and the target itself.
- Produce **one** identifiable executable per platform, or name it with
  `--artifact`. Two candidates in the output directory is an error, not a
  coin flip.

## 3. Match the declared tool versions

The machine installs exactly what [machine.json](machine.json) declares and puts
it first on PATH. A `build` does not read your `.nvmrc` or `rust-toolchain.toml`.

A workflow replay is different: `actions/setup-node`, `actions/setup-go` and
`shivammathur/setup-php` install the release their step declares — through
`node-version`, `go-version`, `php-version` or the matching `*-version-file` —
and put it first on PATH for the steps after them. The archive is checked
against the checksum its publisher lists (nodejs.org, go.dev, getcomposer.org);
PHP comes from where setup-php takes it on a hosted Ubuntu runner: a release the
machine already has is switched to, any other is setup-php's cached build for
this Ubuntu version and architecture (shivammathur/php-ubuntu), checked against
the sha256 GitHub publishes for it and installed through passwordless `sudo`, as
on a runner. A release with no such build, or a declared extension the build
does not carry, fails the step with the reason; nothing else stands in. The
release must run, with its declared extensions, before it becomes `php`; if it
does not, the step fails naming the libraries the machine lacks and the
previous selection is left in place. `coverage: none` disables Xdebug and PCOV
in every SAPI of the release, and `xdebug` (`xdebug3`) or `pcov` enables that
driver and disables the other, as setup-php does; the release's `php -m` after
the step must agree, or the step fails naming the driver. Any other value fails
validation.

Those builds load libraries the runner image carries. The Linux profile of
[machine.json](machine.json) therefore declares the image it stands in for
(`image`: `ubuntu-26.04-arm`, the apt packages of its published toolset and its
PHP 8.5 packages), `setup` installs them and `doctor` checks them. What of the
image the machine does not provide is listed there and recorded as a limit of
every replay of a job on that runner. The image's `/etc/environment` is
declared there too: steps run with its `PATH` (`/usr/sbin` and `/sbin`
included) after the machine's managed tool directories, and with
`DEBIAN_FRONTEND=noninteractive`, `ACCEPT_EULA=Y` and `XDG_CONFIG_HOME`, as on
the image. A replay does not pin Rust either: the workflow's
`rust-toolchain.toml` or rustup call selects it, as on a runner.

Make your project work with the declared Node.js, pnpm, Rust and Go, or change
`machine.json` and say why. `build-machine doctor` reports what is actually
installed against what is declared.

## 4. Account for what each platform needs

| Platform | What the project has to survive |
| --- | --- |
| Windows | WebView2, MSVC, ARM64. A `run:` block runs in PowerShell, which reports only the last line's exit code. |
| Ubuntu | WebKitGTK and the system libraries in `machine.json`'s `packages`. |
| macOS | A universal build has to contain both architectures; the machine checks with `lipo` and fails if one is missing. |

Compiling for the right OS is your code's problem. The machine provisions its
declared prerequisites; it does not infer a native library from your source.

### Put one command in a step

A `run:` block is handed to a different shell on each runner, and they do not
agree on what a failure means. `bash -e` on Linux and macOS stops at the first
command that fails. PowerShell on Windows runs every line and reports only the
last exit code, so a failing type-check followed by a passing test suite is a
passing job.

```yaml
      # Windows reports only the last line's exit code, so the check above it
      # can fail without failing the job.
      - name: Test the frontend types and the Rust core
        run: |
          pnpm exec tsc --noEmit
          cargo test --locked --manifest-path src-tauri/Cargo.toml

      # One command per step fails the same way everywhere.
      - name: Check the frontend types
        run: pnpm exec tsc --noEmit

      - name: Test the Rust core
        run: cargo test --locked --manifest-path src-tauri/Cargo.toml
```

Separate steps also make the report say which one failed, rather than leaving
one block's output to be read.

## 5. The workflow is the contract

`ci validate` and `ci run` read one `.github/workflows/*.yml` file and reproduce
the part of it this machine can actually perform. Anything else fails
validation rather than being quietly changed into something else.

**Supported actions** — `actions/checkout`, `pnpm/action-setup`,
`actions/setup-node`, `actions/setup-go`, `shivammathur/setup-php`,
`dtolnay/rust-toolchain`, `swatinem/rust-cache`, `actions/cache`,
`tauri-apps/tauri-action`, `actions/upload-artifact`,
`actions/download-artifact`, `actions/upload-pages-artifact`,
`actions/deploy-pages`, `softprops/action-gh-release`,
`peter-evans/create-pull-request`. An action's owner and name match without
regard to case, as on GitHub. create-pull-request pushes nothing and opens no
pull request: it records the branch, the base, the title and the files that
changed in the repository of `path` against its checked-out commit, limited to
`add-paths`, with their sizes and SHA-256 (a deleted file has `deleted`), and
inputs other than `token`, `path`, `branch`, `base`, `title`, `body`,
`commit-message` and `add-paths` fail validation. upload-artifact keeps the
files of `path` (one relative path per line, laid out under their least common
ancestor) as the artifact `name` of the run on this machine, and a path that
matches nothing adds none, as the action's default warning does;
download-artifact puts the artifact `name` of the run, or of the earlier
replay that `run-id` names, into `path`. Nothing is sent to GitHub, which each
records as a limit; inputs other than `name`, `path` and `retention-days` for
the upload and `name`, `path`, `run-id` and `github-token` for the download
fail validation, and a download without `name` fails. A setup action's `with` input that its adapter does not
honour fails validation, as do the pages actions' inputs other than `path`,
`name` and `retention-days` for the upload and `artifact_name` for the
deployment. The upload keeps the site as it was when the step ran for the rest
of the replay; deploy-pages deploys nothing and records that artifact's files
with their sizes and SHA-256 in the result's `deployments` as a dry run, and
fails when no pages artifact was uploaded earlier in the replay.
action-gh-release publishes nothing: it takes the tag from the replayed tag or
`tag_name` and fails on a branch without one, as the action does, and records
the files each line of `files` matches, with their sizes and SHA-256, and the
`body_path` file in the step output as a dry run. A `files` pattern uses `*`
and `?` within one path segment; other glob forms fail the step, and a pattern
that matches no file is named in the output, as the action warns about it.
Inputs other than `files`, `body_path` and `tag_name` fail validation. A job's
`environment`, a name or `{name, url}` whose name has no expression and whose
url may read the job's step outputs (recorded as written), is recorded as a
limit and has no local effect.

Shell `run` steps execute as written, in `bash -e` on Linux and macOS as a
runner runs them, and in PowerShell on Windows. Steps run in workflow order.
Each step can set variables through `$GITHUB_ENV` and add to PATH through
`$GITHUB_PATH` for the steps after it; `GITHUB_WORKSPACE`, `RUNNER_TEMP`,
`RUNNER_OS`, `RUNNER_ARCH`, `GITHUB_SHA`, `GITHUB_REF`, `GITHUB_REF_NAME`,
`GITHUB_EVENT_NAME` and `GITHUB_JOB` are set.

**Checkout** — every job starts in an empty `GITHUB_WORKSPACE`
(`$HOME/work/<repository>/<repository>`, as a runner checks out to
`/home/runner/work/<repository>/<repository>`; `RUNNER_WORKSPACE` is
`$HOME/work/<repository>` and `RUNNER_TEMP` lies under `$HOME/work/_temp`), and `actions/checkout`
makes it a Git repository at the replayed revision with the commands the
action runs: one commit by default, every branch and tag at `fetch-depth: 0`,
and the branch or tag of the replay checked out (a commit is checked out
detached). The history is this repository's own branches, tags and `HEAD`,
sent as a Git bundle and fetched through a local mirror that is `origin`; it
is not the GitHub remote, which is recorded as a limit. A replay of the
working tree carries its uncommitted edits, deletions and untracked files
staged on the replayed commit, so `git ls-files` and `git grep` see them and
`git log` sees only real commits; that is recorded as a limit too. `path`
places the checkout in a relative directory under the workspace, which must be
empty or absent; steps without `working-directory` still run in the workspace
root. `repository` checks out another repository that `machine.json`
`repositories` maps to a local clone, at its `ref` (a branch, a tag or a commit
SHA, the clone's `HEAD` when absent), from that clone's committed history; a
`repository` given by an expression, such as
`${{ github.event.pull_request.head.repo.full_name }}`, is read with the
replay's `github` values before it is looked up. `ref` without `repository`
checks out the workflow's own repository at that ref from its committed
history, without the working tree's changes. `submodules` and the other inputs
that would check out something else fail validation. When a job ends, processes that still carry
its `RUNNER_TRACKING_ID` are terminated, as a runner does (Linux), and its
workspace is removed: the artifacts the result names are moved beside it first
and the result points to them there. The `retention` byte cap of
[machine.json](machine.json) is applied before a replay starts as well as after
it.

**Jobs** — a job may use `name`, `runs-on`, `needs`, `env`, `steps`,
`timeout-minutes`, `environment`, `if` and `permissions` (which governs only the
GitHub token a replay does not have); the workflow may use `name`, `run-name`,
`on`, `env`, `jobs`, `permissions` and `concurrency`. Workflow `env` reaches
every step under the job's and the step's own. Each job keeps its own
`$GITHUB_ENV`, `$GITHUB_PATH` and status. A job runs by its `if:` as GitHub reads
it: `success()` when every job before it (the jobs it needs, and theirs)
succeeded, `failure()` when one of them failed, `always()`,
`needs.<job>.result` (`success`, `failure`, `cancelled`, `skipped`) of a job it
needs, and the `github` values a replay has; without an `if` it is
`success()`. A job that does not run is recorded as skipped with the reason,
and the jobs that need it see `skipped`. `needs.<job>.outputs`, step outputs and
other contexts in a job's `if` fail validation. A job cannot need a job of another operating system, because each
operating system is replayed on its own.

**gh** — every replayed job finds the replay's own `gh` first on its PATH
(Linux and macOS), so a step never reaches GitHub through the machine's `gh`.
It answers `gh pr view <number> --json files,headRefOid,number [--jq <filter>]`
from the pull request that `ci run --pull-request <file>` declares, a JSON
object `{number, head_sha, files}`; `gh pr merge <number> --squash` (or
`--merge`, `--rebase`) with `--match-head-commit` equal to the declared head
commit is a dry run recorded as a limit, and a different commit fails. Any
other `gh` command, and `gh pr` without a declared pull request, fails and
names itself.

**Reusable workflows** — a job with `uses: ./.github/workflows/<file>` (and
only `name`, `needs` and `permissions`) runs the jobs of that workflow of the
same repository, which must declare `on: workflow_call`, in its place: they are
named `<calling job>/<called job>`, the first of them wait for what the calling
job needs, a job that needs the calling job waits for all of them, and they see
the caller's `github` values. A replay of a ref reads the called workflow from
that ref. `with`, `secrets` and `if` on the calling job, a workflow of another
repository and nesting deeper than four levels fail validation.

**Time limits** — a job runs for its `timeout-minutes`, or GitHub's 360 minutes
when it declares none, and a step for its own `timeout-minutes` within what is
left of its job's. Nothing else bounds a step. A step past its own limit fails;
a job past its limit is cancelled, as GitHub cancels it, so only steps whose
`if:` runs after a cancellation (`always()`, `cancelled()`) run after it. A step
ends when its process exits: output a background process still holds is read
for five more seconds, as GitHub's runner does, and the process is left
running.

**Not supported** — containers, services, reusable workflows, `strategy`, any
other job key (`continue-on-error`, `defaults`, `outputs`, …), workflow `defaults`, any action without an adapter, and the step keys
`shell` and `continue-on-error`. These fail validation.

**Expressions** — an `if:` may use `success()`, `failure()`, `always()`,
`cancelled()`, literals, `!`, `&&`, `||`, `==`, `!=` and parentheses, inside
`${{ }}` or not. After a failed step, the steps after it run only when their
`if:` says so, as on a runner, so `if: ${{ !cancelled() }}` keeps reporting.

A `run` step with an `id` sets outputs through `$GITHUB_OUTPUT`, and a later
step of the same job reads them as `${{ steps.<id>.outputs.<name> }}` in its
`if:`, `run:`, `env`, `working-directory` and the `with` inputs its adapter
reads. Validation checks that the step exists earlier in the job; the value is
filled in just before the step runs, and an output the step did not write is
empty, as on GitHub. The same places read `github.event_name` (the replay's
event), `github.sha` (the replayed revision), and `github.ref` and
`github.ref_name` (the branch or tag the replay checks out: the `--ref` branch
or tag, or the working tree's current branch). A replay of a commit that no
branch or tag names refuses a workflow that reads `github.ref` or
`github.ref_name`. They also read `github.event.<path>` from the event
payload given with `--event-payload <file>`, a JSON object as GitHub sends
the event; steps receive that file as `GITHUB_EVENT_PATH`, and validation
fails when the payload is missing or does not hold a path that the workflow
reads. An expression that reads any other context fails validation. A condition that
depends on a secret is treated as false and recorded as a limit, so a signing
step is skipped rather than half-attempted.

### The three gates

A workflow must **build**, **test** and **smoke**, or say why it does not:

```yaml
# build-machine: skip smoke reason=the desktop launch is verified separately against a signed-in session
```

The reason is stored in the platform result. This is the point of the gate: a
missing check becomes a decision on the record instead of an omission nobody
noticed.

Prefer a real step over a skip. If your repository already has tests, run them —
that is usually the whole change.

### How a step is assigned to a stage

A step that uses a supported action is assigned by that adapter: checkout,
pnpm/node/Rust setup and caches are `setup`, the Tauri action is `build`,
artifact upload and release are `release`. Only shell `run` steps are read from
their own name and command, matching `smoke`/`launch`/`health`/`e2e`, then
`test`/`lint`/`check`/`verify`, then `build`/`package`/`compile`.

So name your shell steps for what they do. An action's own name never counts —
`actions/checkout` contains "check" and does not make a test stage.

The stage is how a step is reported, not when it runs. Steps run in the order
the workflow writes them, so a step that reads what an earlier step produced —
`make test-servers` and then the environment file it wrote — finds it.

### A workflow that hard-codes one platform can only be replayed there

`runs-on` says where a job was written to run, and an argument like
`--target universal-apple-darwin` says it again. Replaying such a workflow
elsewhere is refused before anything starts:

```
이 워크플로는 macos 에서 실행되도록 작성됐어요. linux에서는 재현할 수 없습니다.
```

`ci validate` reports which platforms a workflow can be replayed on, so you can
see this without running anything.

To rehearse all three, **do not name the target yourself** — the machine
supplies the one for the platform it is building on, from `machine.json`. Only
name it when you mean that one platform and no other.

### What a replay never establishes

Signing, notarization, artifact upload and release publication are replaced by
local adapters and recorded as limits. A successful replay is
`passed_with_limits`, never proof that a real release would publish.

---

## Checklist

```
[ ] The Git repository root is what you register
[ ] Lockfiles are committed
[ ] No external symlinks, no submodules
[ ] The build runs with no prompts and no developer-specific paths
[ ] One identifiable executable per platform, or --artifact names it
[ ] The project works with the versions machine.json declares
[ ] The workflow uses only supported actions, no matrix, no containers
[ ] A build stage exists
[ ] A test stage exists, or a skip comment says why not
[ ] A smoke stage exists, or a skip comment says why not
[ ] build-machine ci validate passes
```

---

## A worked example

AIRDATA is a Tauri 2 application whose release workflow built and published a
macOS app. It carried 38 Rust tests that the workflow never ran, and it had no
smoke step. Validation refused it:

```
ERROR: test 단계가 없어요. 워크플로에 '# build-machine: skip test reason=...' 주석을 추가해야 해요.
```

Two changes satisfied the contract. A test stage, because the tests already
existed and only needed running:

```yaml
      - name: Test the frontend types and the Rust core
        run: |
          pnpm exec tsc --noEmit
          cargo test --locked --manifest-path src-tauri/Cargo.toml
```

And a recorded reason for the stage a hosted runner genuinely cannot perform:

```yaml
# build-machine: skip smoke reason=the desktop launch is verified by `build-machine run` against a signed-in desktop session, which a hosted runner does not have
```

The replay then ran end to end:

```
전체: passed_with_limits
  setup    passed_with_limits   checkout · pnpm · node · rust · cache · install · sign(skipped)
  test     passed               38 Rust tests and the frontend typecheck
  build    passed               tauri-action → data_0.1.0_universal.dmg
  smoke    passed_with_limits   skip smoke=…
```

Replaying the same workflow on Ubuntu then found something the macOS-only
workflow never could: one of the 38 tests failed there. It connected a second
client and read `items[0]` without waiting for the first client's post to be
acknowledged, so it passed only while the machine happened to be fast enough.
The same file already had the pattern for waiting; the test simply had not used
it. With that fixed all 38 pass on both.

That is the whole point of a three-OS rehearsal, and it is also why replaying
this particular workflow on Ubuntu is now refused earlier: its `runs-on` and its
`--target universal-apple-darwin` both say macOS. A workflow meant for three
platforms must not name the target itself.

The tests now run before a tag is cut, which is a change to the project's own
release quality, not a formality this machine imposed.
