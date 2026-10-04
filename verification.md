# Verification

[한국어](verification.ko.md)

The desktop GUI, Windows Node.js 22.23.2 preparation, persistent environment results and known intermittent Parallels execution failure are recorded separately in [GUI-VERIFICATION.md](GUI-VERIFICATION.md).

> **This file records what happened with the Python and PowerShell implementation
> that version 0.1.0 removed.** It is kept as a record of those runs and is not
> edited to describe the Rust implementation, which would make it false. What the
> Rust rewrite has and has not been shown to do is directly below.

## A replay that ran no step — 2026-10-04

Platform: macOS 26.6.2 arm64 (no VM).

- Red, against B5 (`b13a25c`): `cargo test -p build-machine-controller --locked --test worker` failed `a_replay_that_ran_no_step_fails_the_platform`: a fake worker that answers the controller's protocol and reports a success with no step gave `success: true`, `status: PassedWithLimits`, `error: None`. `crates/core/tests/report.rs` did not compile: `error[E0599]: no method named executed_steps found for struct PlatformResult`.
- Green: core report 1/1, controller worker 2/2. `cargo test --workspace --locked` passed 131 tests; `cargo clippy --workspace --all-targets --locked -- -D warnings` passed. `cargo xtask worker --os macos` rebuilt the macOS worker, which answers protocol `688f4a2f99e55910`.

## Worker protocol — 2026-10-04

Platform: macOS 26.6.2 arm64 (no VM).

- Cause: `ci run` of the sidecar-files `ci.yml` (run `20261004-164249-644878`) ran `workers/build-machine-worker-macos` built on 2026-09-10. It provisioned tools, ran no workflow step and reported `passed_with_limits` in 21 s; nothing compared the worker with the controller.
- Red, against B4 (`4258877`): `cargo test -p build-machine-controller --locked --test worker` failed `a_worker_of_another_protocol_fails_the_platform`: a fake macOS worker that prints a successful report with no step for any arguments was run, and the platform result was `success: true`, `status: PassedWithLimits`, `error: None`. `cargo test -p build-machine-core --locked --test request` failed `a_request_of_another_protocol_is_refused`: `WorkRequest::load` accepted a request with `"protocol": "stale"` (`called Result::unwrap_err() on an Ok value`).
- Green: both pass. The controller's error is `macos 워커의 protocol BUILD_MACHINE_REPORT_END이 controller의 protocol f3de022a2b5074bb과 달라요. 워커를 현재 소스로 다시 빌드해야 해요: cargo xtask worker --os macos`. `cargo test --workspace --locked` passed 129 tests; `cargo clippy --workspace --all-targets --locked -- -D warnings` passed. `ci validate` of the sidecar-files `ci.yml` returned `"status": "valid"`.
- `cargo xtask worker --os macos` rebuilt `workers/build-machine-worker-macos` (`x86_64 arm64`) after `rustup target add x86_64-apple-darwin`; `build-machine-worker-macos protocol` printed `f3de022a2b5074bb`, the controller's protocol at this commit. The Linux and Windows workers in `workers/` predate this protocol and fail their platform until they are rebuilt in their virtual machines.

## Pages deployment and job environment — 2026-10-04

Platform: macOS 26.6.2 arm64 (no VM and no `ci run`).

- Red, against B3 (`3d6ae63`): `cargo test -p build-machine-core --locked --test workflow` failed 3 of 30 with `job publish의 'environment'는 아직 지원하지 않아요.`: `a_pages_deployment_and_its_environment_pass_validation` (`called Result::unwrap() on an Err value`), `an_environment_without_a_literal_name_fails_validation` and `a_deploy_pages_input_it_does_not_honour_fails_validation` (message assertions). `crates/worker/tests/pages.rs` did not compile: `error[E0432]: unresolved import build_machine_worker::pages`.
- `a_job_key_the_replay_does_not_implement_fails_closed` (crates/core/tests/job.rs) listed `environment: production` as refused; it is now among the accepted keys, as this change defines.
- Green: core workflow 30/30, worker pages 3/3 (the deployment lists the files as uploaded, without `.git`, with sizes and SHA-256; a deployment without an upload, a missing upload path and a second upload of one name fail). `cargo test --workspace --locked` passed 127 tests; `cargo clippy --workspace --all-targets --locked -- -D warnings` passed. The worker wiring (the step adapters, the environment limit and `deployments` in the result) has no test of its own; it is exercised by a replay.
- `ci validate` of the registry `.github/workflows/publish.yml` (`261104c4370f3e7fdb14eedb8b9b30077aa5e9e1`, dirty), `--event push --os macos`, returned `"status": "valid"` with stages build 2, release 2, setup 9, smoke 1, test 1 and the six soksak repositories.

## Own checkout into a path — 2026-10-04

Platform: macOS 26.6.2 arm64 (no VM and no `ci run`).

- Red, against B2 (`d51f7ff`): `cargo test -p build-machine-core --locked --test workflow` failed 1 of 27: `the_own_repository_is_checked_out_into_a_path` with `called Result::unwrap() on an Err value: job test의 step 3 (actions/checkout@v4): checkout의 path는 repository와 함께 다른 저장소를 checkout할 때만 지원해요.` `cargo test -p build-machine-worker --locked --test checkout` failed 1 of 11: `the_own_repository_is_checked_out_into_a_path` with `called Result::unwrap() on an Err value: No such file or directory (os error 2)`, because the checkout required an existing directory.
- `checkout_inputs_are_read_or_refused` (crates/core/tests/job.rs) listed `path: src` as refused; it now accepts `path: src` and refuses `path: ../src`, as this change defines. `ref_and_path_without_a_repository_fail_validation` became `a_ref_without_a_repository_fails_validation`.
- Steps without `working-directory` already run in the workspace root: the worker passes `GITHUB_WORKSPACE` as the step directory, whatever `path` a checkout used.
- Green: core workflow 27/27, worker checkout 11/11. `cargo test --workspace --locked` passed 121 tests; `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `ci validate` of core `.github/workflows/ci.yml` (`f9929097c3e72250498e3700b86b233c02ead99e`, dirty) and the registry `.github/workflows/ci.yml` (`261104c4370f3e7fdb14eedb8b9b30077aa5e9e1`, dirty), `--event push --os macos`, both returned `"status": "valid"`. core: stages build 1, setup 10, smoke 1, test 3, repositories plugin-browser, plugin-files, plugin-terminal, sidecar-files and sidecar-vt. registry: build 1, setup 5, smoke 1, test 1, repositories plugin-browser, plugin-files and plugin-terminal.

## The github context — 2026-10-04

Platform: macOS 26.6.2 arm64 (no VM and no `ci run`).

- Red, against the B1 implementation (`b74e9ae`): `cargo test -p build-machine-core --locked --test workflow` failed 1 of 26: `the_github_values_of_a_push_pass_validation` with `called Result::unwrap() on an Err value: job release의 step 2 (actions/checkout@v4)의 with ref: Unsupported workflow context: github.ref_name. …`. `cargo test -p build-machine-controller --locked --test replay` failed 1 of 3: `a_replay_of_a_commit_without_a_ref_name_is_refused_when_the_workflow_reads_one` with the same error. `crates/core/tests/expression.rs` did not compile: `error[E0432]: unresolved import build_machine_core::workflow::Github`, `error[E0425]: cannot find function github_context in module workflow`, `error[E0061]: this method takes 1 argument but 2 arguments were supplied` (`Template::render`). `any_other_github_value_still_fails_validation` passed before and after; it guards the names that stay unsupported.
- Existing tests that used `github.ref`, `github.ref_name`, `github.event_name` or `github.sha` as examples of a context without a value now use `github.actor`, `github.repository` and `github.event.pull_request.merged`, because the four names have values by this change.
- Green: core expression 6/6, core workflow 26/26, controller replay 3/3. `cargo test --workspace --locked` passed 119 tests; `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `build-machine ci validate /Users/maxkwon/Projects/soksak/sidecars/files --workflow .github/workflows/release.yml --event push --os macos` (sidecar-files `3c5863aefb8a161832542faa9b0d29af65a40fb8`, dirty, on `refs/heads/main`) returned `"status": "valid"` with stages build 2, release 1, setup 3, smoke 1, test 1, platform `macos` and repositories `soksak-app/core`. Without `--ref`, `github.ref_name` is `main`, so a run checks out `soksak-app/core` at `main`; a release rehearsal passes `--ref <tag>`.

## Checkout of another repository — 2026-10-04

Platform: macOS 26.6.2 arm64 (controller and macOS worker code on the host; no VM and no `ci run`).

- Red, against the unchanged implementation: `cargo test -p build-machine-core --locked --test workflow` failed 5 of 24 tests. `a_checkout_of_another_repository_is_read_with_its_ref_and_path` failed with `called Result::unwrap() on an Err value: actions/checkout@v4의 with 입력 'path'은 아직 지원하지 않아요`; `a_checkout_path_outside_the_workspace_fails_validation`, `ref_and_path_without_a_repository_fail_validation`, `a_checkout_ref_without_a_local_value_fails_validation` and `a_checkout_repository_must_be_owner_and_name` failed their message assertions with the same unsupported-input error. `cargo test -p build-machine-controller --locked --test replay` failed both tests with that error: `a_replay_bundles_every_repository_its_checkout_steps_name` and `a_repository_the_machine_does_not_map_fails_validation`. The worker test `crates/worker/tests/checkout.rs` did not compile: `error[E0432]: unresolved imports build_machine_worker::checkout::checkout_repository, build_machine_worker::checkout::RepositoryCheckout`.
- Green: the core workflow tests passed 24 of 24, the controller replay tests 2 of 2 and the worker checkout tests 10 of 10. The worker tests check out a tag into `core` beside existing workspace files without the clone's uncommitted edit, a branch at `fetch-depth: 0`, a commit SHA, the bundle's `HEAD`, and an unknown ref and an occupied directory as explicit failures. `another_repository_is_checked_out_at_an_annotated_tag` was added after the implementation and is not Red evidence.
- `cargo test --workspace --locked` passed 113 tests, and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `build-machine ci validate /Users/maxkwon/Projects/soksak/sidecars/files --workflow .github/workflows/release.yml --event push --os macos` (sidecar-files `3c5863aefb8a161832542faa9b0d29af65a40fb8`, dirty) failed: `job release의 step 2 (actions/checkout@v4)의 with ref: Unsupported workflow context: github.ref_name. Only steps.<id>.outputs.<name> has a value in a local replay.` The `github` context has no local value yet (B2). The workflow also declares no smoke step and no smoke skip comment, which the gate refuses after the expression is accepted.

## Local environment recovery — 2026-09-30

See [environment setup and recovery](ENVIRONMENT.md) for the repeatable procedure and GitHub Actions compatibility limits.

- Ubuntu 26.04 ARM64 was newly created with desktop account `parallels` and Parallels Tools 27.0.2 (58673). `prlctl exec --current-user` failed authentication despite a signed-in desktop. The Linux controller now uses the privileged Parallels channel to run the worker through `runuser -l <desktopUser> -c ...`, without storing a password. Windows execution is unchanged.
- Native macOS GUI was built and opened with its Vite development server; the dashboard rendered. This is a visual startup check, not an automated test of every GUI action.
- Existing Linux setup run `20260930-183353-394136` succeeded. Subsequent doctor run `20260930-183516-469377` reported `ready=true`, no missing packages and no issues. Actual tools: Node 22.23.2, pnpm 11.24.0, Rust 1.98.1, Go 1.27.1 and Ubuntu Git 2.53.0.
- `cargo test --locked -p build-machine-core -p build-machine-controller` passed 27 tests, including a new two-shell argument preservation test with spaces, quotes, newlines and shell metacharacters. Controller clippy with all targets and warnings denied passed after installing the missing host clippy component.
- AIRDATA revision `010c133782119c0ffb4e541c9727b6f4442f79db` (clean), workflow `.github/workflows/release.yml`, passed local workflow validation. Linux replay run `20260930-183524-943541` finished with `passed_with_limits` at 2026-09-30 18:42 KST. Dependency installation, frontend type checks, all 38 AIRDATA Rust tests, frontend production build and ARM64 DEB packaging passed. The workflow explicitly skips smoke; GitHub secrets are empty locally and signing/publication are unverified. DEB size: 5,353,902 bytes; SHA-256: `fb536dd78c6836d72f579fa35f54ea62bff9aa74a1d3f9819f5da519ae27757b`. No system-level DEB installation or AIRDATA launch was performed in this run. Its workflow requests Ubuntu 24.04 ARM64 while the guest is Ubuntu 26.04 ARM64; local success does not establish runner-image equivalence.
- The first CLI replay omitted `--root`, selected the staged `target/debug` payload and failed the existing-share directory check before running workflow steps. It was corrected with an explicit controller root. A preceding doctor encountered a Parallels session-creation error; later independently invoked setup and doctor completed. Neither failure was hidden or automatically retried.

## Rust rewrite

The rewrite passes `cargo test --workspace` (40 tests) and `cargo clippy --workspace --all-targets -- -D warnings` on the macOS host, `pnpm --dir gui run build`, and `pnpm --dir gui test` (14 browser tests). The worker compiles with no warnings for `aarch64-apple-darwin`, `aarch64-unknown-linux-gnu` and `aarch64-pc-windows-msvc`.

### Exercised against the actual virtual machines

Guests: `Windows 11` (Windows 11 Pro ARM64) and `Ubuntu 26.04 ARM64`, both running with a desktop user signed in. Project: `/Users/maxkwon/Work/airdata`, revision `c798dc27c569fdcd4ab83a6dad5722ab739f41f0`, source hash `39a73fa25283aabe62c239a5ff870ac19435a9868ea9e911b689dfe6d1e04eef`.

- `cargo xtask worker --os windows linux` built each guest worker inside its own virtual machine and returned it. Cargo reads the workspace straight from the read-only share and writes only to a target directory in the guest, so nothing is copied in.
- `doctor` passed on all three environments, both individually and as one matrix. Windows reported `Windows 11 Pro`, `ARM64`, MSVC `17.14.37628.2` and WebView2 `152.0.4191.66` — the registry reads, the vswhere query and the architecture check are the new direct Win32 calls.
- `build` passed on Windows, Ubuntu and macOS. Ubuntu produced the ARM64 executable and `data_0.1.0_arm64.deb`; macOS produced a genuine universal application, `lipo -archs` reporting `x86_64 arm64`. Repeating the Ubuntu build reported `REUSED BUILD` with an unchanged executable checksum.
- `run` passed on Windows and Ubuntu, and repeating it reused the running process. On Windows the window checks reported the title `data` and `responding: true`; `.state/…/windows-app.png` shows the application rendering its Korean board screen.
- `ci validate` rejects `airdata/.github/workflows/release-macos.yml` for the missing **test** gate, matching the corrected contract: `actions/checkout` no longer stands in for a test step.
- One matrix build recorded Ubuntu as failed with `PrlJob_GetResult: Invalid argument`. The identical command succeeded when repeated, and two further controller runs passed. This is the intermittent Parallels 27.0.1 execution failure recorded below; the run reported it rather than retrying, which is the intended behaviour.

### Defects this exercise found and fixed

Every one of these was found by running the code, not by reading it:

- The machine/user privilege split had not been carried over. System-wide installation needs rights the desktop user does not have, so the controller now runs `setup-system` elevated — as SYSTEM on Windows, root on Linux — before the work the desktop user must do. Elevation is required only when something actually has to be installed.
- A launched Windows application stayed inside the job object `prlctl exec` creates, so the controller waited for the application to exit and never returned. The launch is handed to the shell, exactly as double-clicking would, which is where PowerShell's `Start-Process` had been going. Linux reaches the same place with `setsid`.
- Paths were built by joining strings containing `/`, producing mixed separators on Windows. The shell would not launch such a path and the running-process match could not compare equal to it.
- `--bundles none` is not a value Tauri accepts on Windows; a development build there takes `--no-bundle`.
- `prlctl exec` does not preserve argument quoting — it joins what it is given and the guest re-parses — so a multi-statement script was split at its first `;`. Every guest step is now one plain command.
- The guest build copied the whole share, including `.git`, into a 1.7 GB tmpfs and exhausted it; the copy also preserved the share's read-only permissions, leaving files that could not be removed.
- The registry's `ProductName` still reads "Windows 10" on Windows 11, so the diagnosis reported the wrong system. The build number decides now.

### The application bundle

`cargo xtask build` produced `Build Machine.app`. It carries the macOS worker as a signed-eligible sidecar in `Contents/MacOS` (`lipo -archs`: `x86_64 arm64`) and the Windows, Ubuntu and macOS workers plus `machine.json` as resources.

- The application launched and rendered its dashboard natively, showing the recorded runs, their platforms and the per-stage step output.
- Copied outside the workspace and launched, it placed its bundled payload into `~/Library/Application Support/local.buildmachine.desktop/machine` — machine definition and all three workers — because the bundle cannot be written to and cannot be shared into a virtual machine.
- The controller then passed `doctor --os macos` against that seeded directory using only the bundled worker. A released application does not need this checkout.

Two defects were found and fixed here: the bundled payload was shipped but never used, because the application fell back to guessing `~/Work/build-machine`, and `xtask` did not pass its toolchain down to Tauri's own CLI, which shells out to `cargo`. `universal-apple-darwin` is a Tauri bundling target rather than one rustc builds, so the macOS worker is compiled per architecture and joined, as the release runner does.

Fifteen run records written by the removed Python implementation were deleted. The current reader cannot parse them and reported them as unreadable on every refresh; there is no compatibility path for them, and `verification.md` is where those runs are recorded.

### Workflow replay

`ADOPTING.md` was written as the guide a project follows to qualify, and AIRDATA was the first project taken through it. Its release workflow carried 38 Rust tests it never ran and had no smoke step, so validation refused it. Adding a test step that runs the tests the repository already had, and a recorded reason for the smoke stage a hosted runner cannot perform, satisfied the contract.

`build-machine ci run ~/Work/airdata --workflow .github/workflows/release-macos.yml --os macos` then completed as `passed_with_limits`:

- `setup` passed with limits — checkout, pnpm, Node.js, Rust and cache through local adapters, `pnpm install --frozen-lockfile` executed, and the signing step skipped because its condition depends on a secret.
- `test` passed — 38 Rust tests and the frontend typecheck, run inside the snapshot.
- `build` passed — `tauri-apps/tauri-action` produced `data_0.1.0_universal.dmg`, recorded with its SHA-256 and size.
- `smoke` passed with limits, carrying the reason from the workflow comment.
- `signing: unverified`, and the limits record that nothing was signed, notarized or uploaded.

Two defects were found and fixed by this run. The `adapter` field was marked `#[serde(skip)]`, so every step arrived at the worker as a shell step and the first one failed on a command that was never there; it is serialized now, and a test asserts every adapter survives the request document. And the machine appended its own `--target` to a workflow whose own `args` already named one; the workflow's arguments win now.

Replaying the same workflow on Ubuntu found a test that fails only there: it read `items[0]` without waiting for the post to be acknowledged, and passed on macOS purely on speed. The same file already carried the pattern for waiting. With that fixed, all 38 tests pass on both.

That run also showed the machine failing in the wrong place. The workflow's `runs-on` is `macos-14` and its `args` name `universal-apple-darwin`, so replaying it on Ubuntu could never work — but the refusal came from `rustup` deep inside the build. A replay on a platform the workflow was not written for is now refused before any environment is touched, and `ci validate` reports which platforms a workflow can be replayed on.

AIRDATA's workflow and test changes are in its working tree and have not been committed there.

### Three operating systems, end to end

AIRDATA now carries `release.yml` with a job per operating system and no hard-coded target. Every platform replayed its own job:

| | replay | release rehearsal | package |
| --- | --- | --- | --- |
| Windows | `passed_with_limits` | `passed` | `data_0.1.0_arm64-setup.exe` |
| Ubuntu | `passed_with_limits` | `passed` | `data_0.1.0_arm64.deb` |
| macOS | `passed_with_limits` | `passed` | `data_0.1.0_universal.dmg` |

Each release rehearsal opened the package it produced: the disk image was mounted and the application copied out of it, the `.deb`'s metadata and contents were read, and the Windows installer was confirmed to be an executable image without being run. None of them installs anything system-wide, and each receipt says so.

Defects this found, all in code written for this machine:

- The replay ignored `runs-on` and ran every job's steps on every platform, so a workflow with a job per operating system could not be replayed at all. Stage gates are checked per platform now too: testing on macOS and not on Linux used to pass validation while the Linux rehearsal quietly ran nothing.
- The macOS disk-image bundler moves the application inside the image, so locating it before opening the package could never work for a release. The package is opened first.
- The package check itself had been dropped in the rewrite without being recorded. It is back on all three platforms.
- `canonicalize` returns an extended-length path on Windows, so every containment check comparing a resolved path against an unresolved base failed there.
- The `dpkg-deb` listing writes paths without a leading slash, which the first version of the Linux check did not expect.

### A desktop session is a prerequisite, so the machine provides it

Builds and launches run as the signed-in user, and a machine that has just been installed or restarted sits at its login screen with no session for `--current-user` to attach to. That was being fixed by hand, which is not provisioning.

`setup-system` now ensures it: the desktop user is declared in `machine.json`, autologin is configured if it is not already, and the display manager is restarted only when nobody is signed in. Running it again reports "No change" and "No restart". `doctor` reports the session as a requirement like any other, and building a guest worker no longer needs a session at all — it becomes the user directly, because compiling does not need a desktop.

It also keeps the desktop from blanking or locking. A launch is verified by looking at the screen, and a machine nobody is sitting at would otherwise turn its screen off a few minutes in, making that impossible.

Restarting the Linux guest was then enough: it came back with a session on its own, no one signed in, and `build-machine run ~/Work/airdata --os linux` launched the application. `.state/ubuntu-app.png` shows it rendering its Korean board screen. That is the last of the three that had not been seen.

The parsing this depends on is pinned by tests, including the case that caused the original mistake: the shipped configuration file carries the settings commented out as examples, and reading one of those as configuration is what leaves a machine at the login screen while provisioning reports success.

### Not exercised

- A system-level installation of any package. Each rehearsal opens the package it built; none installs it.
- The release workflow. It has never run; no runner has built a worker and no application has been assembled from one.
- Driving a guest from the seeded application directory. Only one directory can hold the named Parallels share, and repointing it would have taken the share away from this checkout.
- Provisioning that actually installs something. Every environment already satisfied `machine.json`, so the installation paths — MSVC, WebView2, apt, the managed toolchains — reported "no installation" and did not run.

## Defect fixes

Eight recorded defects were fixed and each is pinned by a test that fails without the fix. On the macOS host `python3 -m unittest discover -s tests` passes 41 tests, `~/.cargo/bin/cargo test --manifest-path gui/src-tauri/Cargo.toml` passes 16 (the configured Ubuntu doctor test remains ignored), `pnpm --dir gui test` passes 14 browser tests and `pnpm --dir gui run build` passes the TypeScript and Vite build.

What these tests establish and what they do not:

- The Windows `setup` crash is covered by a controller test with a stub machine object. The guest path it would have taken has still never been run: Windows provisioning was only ever exercised through `winbuild.py setup`, and `build.py setup --os windows` has not been run against the actual VM.
- Retention, the run layout and the legacy migration are covered by tests over a temporary state directory. The migration was additionally run once against this checkout's own `.state`: 20 earlier reports moved under `.state/runs/` with their recorded times preserved and no legacy file left behind.
- The operation log now contains the run summary and the location of each platform's command output. This was confirmed with a controller run whose worker was a local process, not a guest.
- Step streaming, exit codes and timeout process-group termination are covered by tests over real child processes on macOS. Streaming through `prlctl exec` into a guest has not been re-run.
- The dashboard changes are covered by Rust tests over report fixtures and by browser tests over mocked desktop APIs. They do not establish native rendering; no rebuilt macOS app was inspected for this change.
- The stage-gate fix was checked against the maintained `airdata/.github/workflows/release-macos.yml`. Before the fix `ci validate` rejected it for the missing **smoke** gate alone, because its `actions/checkout` step was being read as the test stage. After the fix it is rejected for the missing **test** gate. The workflow has neither step, so the earlier record of it being rejected was correct in outcome but the test gate was never actually enforced.

No AIRDATA source, GitHub workflow, signing credential, upload or release was changed or invoked.

## Workflow replay implementation

The workflow replay implementation adds a strict parser and local runner for the supported GitHub Actions subset. Its tests cover explicit unsupported-action and missing-gate failures, immutable ref snapshots, repository-root validation, sequential/parallel report equivalence and native workflow stage execution. Current test counts are recorded under "Defect fixes" above; the counts first recorded here were 23 Python, 10 Rust and 13 browser tests.

The maintained AIRDATA workflow currently has no test or smoke step and no `build-machine: skip ... reason=...` comments. `ci validate` therefore rejects it with the required missing-gate message until the repository documents those omissions. No AIRDATA source, GitHub workflow, signing credential, upload or release was changed or invoked by these checks. Windows and Ubuntu workflow replay code is implemented and covered by controlled tests; live replay still requires an actual run on each configured guest.

The Windows and Ubuntu build and launch baselines passed on 2026-09-09. Three-OS release rehearsal, installer verification, and GitHub Actions parity are not complete.

- Host: macOS ARM64; Parallels Desktop 27.0.1 (58670).
- Guest: Windows 11 Pro ARM64; Parallels Tools 27.0.1 (58670).
- Host-to-guest commands work as the signed-in desktop user and as SYSTEM for machine-wide prerequisites.
- MSVC installation completed with installer exit code 0. All required components were confirmed by `vswhere`.
- Initial application: `/Users/maxkwon/Work/airdata`, Git revision `c798dc27c569fdcd4ab83a6dad5722ab739f41f0`.
- Wails projects and custom recipes have not yet been exercised.

## Verified commands

- `python3 -m unittest discover -s tests -v`: five tests passed. They cover current uncommitted files, ignored data exclusion, deterministic source hashing, tracked deletion, external symlink rejection, and project directory separation.
- `python3 winbuild.py setup`: passed. A second run passed with every tool reused and no PATH change.
- `python3 winbuild.py build /Users/maxkwon/Work/airdata --run`: passed. The frontend production build and Windows Rust release build completed; Rust reported two existing dead-code warnings.
- Repeated the same build/run command: `REUSED BUILD`, unchanged executable checksum, and `reusedProcess: true` for PID 6160.
- A Parallels screen capture showed the application window titled `data` and its rendered Korean board screen. Screenshot: `.state/airdata-windows.png`.

## Actual environment and artifact

| Component | Verified version |
| --- | --- |
| Node.js | 24.21.0 |
| pnpm | 11.24.0 |
| Rust | 1.98.1, `aarch64-pc-windows-msvc` |
| Go | 1.27.1, Windows ARM64 |
| Git | 2.55.0.windows.5 |
| MSVC | 17.14.37628.2 |
| WebView2 | 152.0.4191.66 |

Controller SHA-256: `9e4fc83afcd5f40d1f0b4d4821fa4894db462d175044d64bb24dfc449844f20a`.

Source archive SHA-256: `a96eac8b2943dc7e465b79025a287d3d16e58a725666a56af2b4c5669d283a77`.

Executable SHA-256: `B15188F530636F41117C0F0C9D0C24C3DAE7A730B20CD74A9609F9051635098D`.

Windows receipt: `C:\BuildMachine\projects\airdata-aa700a5858\latest.json`.

Host logs: `.state/logs/20260909-183330-926932.log` (second setup), `.state/logs/20260909-183410-083285.log` (build and launch), `.state/logs/20260909-183826-662673.log` (repeat).

## Release coverage still required

`airdata/.github/workflows/release-macos.yml` currently defines only macOS: `macos-14`, Node.js 22, pnpm 11, stable Rust, `universal-apple-darwin`, conditional Apple signing/notarization, and publication through `tauri-action`. The original verified Windows baseline uses Node.js 24 and skips installer bundling. It does not reproduce that workflow, signing, publication, x64 Windows, or macOS acceptance. The current configuration selects Node.js 22.23.2 to match the workflow's major version; that configured version has been exercised on Linux and in macOS provisioning, but not yet in a Windows build. No GitHub workflow was dispatched and nothing was published.

## Ubuntu 26.04 ARM64

- Guest: `Ubuntu 26.04 ARM64`, Ubuntu 26.04 LTS, Python 3.14.4, Parallels Tools 27.0.1 (58670), desktop user `parallels`.
- Missing native packages were installed through the maintained `native.py setup-system` command as root. Node.js 22.23.2, pnpm 11.24.0, Rust 1.98.1, and Go 1.27.1 were installed through `native.py`; Git is Ubuntu's 2.53.0. Repeated preparation reported no missing packages and reused every declared user tool.
- `python3 build.py build /Users/maxkwon/Work/airdata --os linux --run` passed. It produced the ARM64 release executable and `data_0.1.0_arm64.deb`, then launched PID 24770. The first release compilation took 3m 08s and reported the same two existing dead-code warnings as Windows.
- `python3 build.py run /Users/maxkwon/Work/airdata --os linux` passed with `reusedProcess: true`, PID 24770. `.state/airdata-linux.png` shows the `data` application window and rendered Korean device-connection screen. The process was left running.
- The visible launch used worker signature `2cae11447ff599760bc7b9db4c2fc0faa469d92c48639b4e49eb15ed15c164b3` and executable SHA-256 `160f467ecf77e3537289d82da4fd1a9db497475eaa34aaa61dced9a402b5bcae`.
- The updated worker, including executable source-mode preservation, passed an actual build and a repeated build without launch. Its signature is `e3961d278550d9d91f1ca63a8224c180f41c88d4f36650c7ca82915ab9cf33c0`; the repeat reported `REUSED BUILD`. Executable SHA-256: `ad226405797b8cb6c16c55ae493e078e39cbc8bcdd1af920dc8a350fd9ea4b1d`. DEB SHA-256: `9fa17dea38b61de011c98a4c9960cdd35c22aad39bf0c3b930ad8d82cfe0b9c2`. The earlier visible process remains open; the latest receipt points to this updated worker's build. Both use the unchanged AIRDATA revision and source archive recorded above.
- Guest latest receipt: `/home/parallels/.local/state/build-machine/airdata-aa700a5858/latest.json`.
- Host logs: `.state/logs/20260909-190210-667721-matrix.log` (provision, build, launch), `20260909-190725-200878-matrix.log` (process reuse), `20260909-190745-429176-matrix.log` (updated worker build), and `20260909-191155-020353-matrix.log` (build reuse), all under `.state/logs/`.
- Ten tracked tests passed on macOS and Ubuntu. They cover snapshot content/isolation, archive validation, executable source permissions, actual build-cache reuse and tamper detection, and full executable-path process lookup using a compiled test program.
- AIRDATA's source repository remains unchanged. Its current `current_platform()` implementation returns `Macos` for every desktop OS, so Ubuntu and Windows device labels display `Mac`. This is a product limitation, not evidence that the Linux build ran on macOS. Device pairing, content transfer, a system-level DEB installation, production signing and release publication were not acceptance-tested.

## macOS provisioning

The native setup passed with private Node.js 22.23.2, pnpm 11.24.0, Go 1.27.1 and the existing Rust 1.98.1 toolchain. The ARM64 and Intel macOS Rust targets are installed. Apple Command Line Tools provide the compiler and SDK. Existing system Node.js and Go installations were preserved. The macOS AIRDATA universal build, DMG and launch have not yet been exercised by this worker.
