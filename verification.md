# Verification

[한국어](verification.ko.md)

The desktop GUI, Windows Node.js 22.23.2 preparation, persistent environment results and known intermittent Parallels execution failure are recorded separately in [GUI-VERIFICATION.md](GUI-VERIFICATION.md).

> **This file records what happened with the Python and PowerShell implementation
> that version 0.1.0 removed.** It is kept as a record of those runs and is not
> edited to describe the Rust implementation, which would make it false. What the
> Rust rewrite has and has not been shown to do is directly below.

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

### Not exercised

- Workflow replay (`ci run`) on any platform. Only `ci validate` has been run.
- `release`, and any installer or package acceptance.
- The release workflow. It has never run; no runner has built a worker and no application has been assembled from one.
- The rebuilt macOS application bundle, its sidecar staging and the dashboard rendering natively.
- Visible rendering on Ubuntu. The process launch and reuse were verified, but the guest screen was locked, so the window itself was not seen.
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
