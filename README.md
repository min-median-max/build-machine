# Build machine

[한국어](README.ko.md)

This repository provides inspectable build-machine commands that run without Codex. The [three-OS release rehearsal specification](PLATFORMS.md) defines the shared objective. Each operating system shares its declared tools across projects and stores sources, outputs and logs in separate project directories.

It is one Rust workspace. The controller runs on this Mac; a worker binary runs on each target. The machine that builds your project needs no Python, no PowerShell and no Rust toolchain of its own — it runs the worker the release shipped.

## Required behavior

- `setup` checks the declared versions and required MSVC components, installs missing tools, and preserves existing installations and application data. Running it again does not reinstall tools that satisfy the configuration or add duplicate PATH entries.
- `doctor` reports the actual selected environment and missing requirements.
- `build` runs the same prerequisite checks and installation procedure automatically. A separate remembered setup step is not required.
- `build PROJECT` copies current Git tracked files and nonignored untracked files, including uncommitted edits. It excludes ignored files and rejects symlinks outside the project. It records the Git revision, source checksum, tool versions, build command, log path and executable checksum.
- Source snapshots are stored separately by content. Builds occur on the selected operating system's local disk. A successful result is reused only when its source, recipe, worker binary, machine configuration and executable checksum match. The worker preserves executable permissions on source scripts.
- `run PROJECT` starts the last successful executable in the signed-in desktop. Repeating it reuses that executable's running process. A process/window check verifies launch; it does not certify all application features.
- Standard Tauri 2 projects use their locked dependencies and produce a release executable. Other layouts can supply an explicit build command and executable path.
- Commands and logs are inspectable. The source is public, the release is built by the public workflow in this repository, and every run records what it ran and where its output went.
- A registered project is the Git repository root. Monorepos select a sub-application through a workflow step's `working-directory`; a subdirectory cannot be registered as its own project.
- `ci validate` reads the repository's GitHub Actions workflow and fails closed for unsupported actions, containers, services or expressions. `ci run` executes supported `uses` and `run` steps in the selected OS workers, with local adapters for checkout, caches, artifacts, releases and signing.
- Workflow replay accepts the current working tree or an explicit commit, branch or tag. It records the resolved revision, dirty state, event, step commands, stage results, artifact checksums and limits. Test and smoke stages require an explicit `# build-machine: skip <stage> reason=...` comment when the workflow does not contain them. A step that uses a supported action is classified by that adapter, so an action's own name never satisfies a stage gate.

## Commands

The [Tauri 2 desktop app](GUI.md) opens with a dashboard of registered projects, their latest recorded build results and recent build logs. Use `+` to register a Git folder, then select its entry to configure build targets, build and launch. Each project retains its own options. The bottom **Settings** button opens Windows/Ubuntu/macOS diagnosis and automatic tool setup shared by all projects. [Adopting the build machine](ADOPTING.md) is what to change in your own repository; [project build requirements](PROJECTS.md) describe the supported layouts and remaining framework coverage.

Registration and GUI settings live in `~/Library/Application Support/local.buildmachine.desktop/preferences.json`. Use **Settings → Open settings folder** to inspect that file.

```sh
cd ~/Work/build-machine
cargo xtask build --run
```

`cargo xtask dev` starts the frontend and the Rust application together; `cargo xtask test` runs the Rust tests, clippy, the frontend build and the browser tests. `cargo xtask worker --os windows linux` builds the guest workers inside their virtual machines, which is how an end-to-end check runs without waiting for a tagged release.

Requirements on the Mac: Git, Rust and Parallels with working `prlctl exec` and Parallels Tools. The selected VM must be running with a desktop user signed in. The VM names and tool versions are in [machine.json](machine.json).

The controller selects `windows`, `linux`, `macos`, or all three when `--os` is omitted. Matrix execution is sequential by default; `--execution parallel` runs selected operating systems concurrently while preserving the same stage, command, log, retry, artifact and failure fields. A multi-platform operation captures one source snapshot, waits for every selected platform, records each result and fails overall if any platform fails. `--result-file PATH` writes the structured result for other interfaces. Diagnosis/setup results and timestamps persist in `.state/tool-status.json`; changed machine configuration invalidates those displayed results.

```sh
build-machine doctor --os linux
build-machine setup --os linux
build-machine build ~/Work/airdata --os linux --run
build-machine run ~/Work/airdata --os linux

# Reproduce a repository workflow. Omitted test or smoke gates require the
# explicit skip comment described above.
build-machine ci validate ~/Work/airdata --workflow .github/workflows/release-macos.yml
build-machine ci run ~/Work/airdata --workflow .github/workflows/release-macos.yml --ref v0.1.0 --execution parallel --os macos
```

`build` installs missing prerequisites automatically. Linux system packages are installed as root through Parallels, while builds and launches run as the signed-in desktop user. The launch command reads only the desktop connection settings from that user's systemd environment. It checks that the process remains running. A separate screen capture verifies visible rendering.

Custom projects supply `--framework custom --command 'BUILD COMMAND' --artifact 'RELATIVE/PATH'` to `build`. Custom commands run in the selected platform's shell as the desktop user inside that project's snapshot.

The machine uses a read-only Parallels share named `WindowsBuildMachine` for transfer. The guest reads its worker binary and source snapshot from that share; nothing is written back through it. Local transfer files, execution records and logs are under `.state/` and excluded from Git. Each run is recorded at `.state/runs/<run>/report.json` with its logs at `.state/logs/<run>-*.log`; the `retention` policy in [machine.json](machine.json) bounds them together by age, per-project count and total bytes, discarding the oldest runs first. The same byte cap bounds the build directories each worker leaves on the machine it builds on, which is where the space actually goes: one of them is a whole dependency tree. The build in progress and the one the latest receipt names are never removed. Windows projects and their build receipts are under `C:\BuildMachine\projects`. MSVC is installed at `C:\BuildTools`; user tools are under `%LOCALAPPDATA%\WindowsBuildMachine`.

MSVC uses Microsoft's serviced Visual Studio 2022 channel and verifies required components. Other tool versions and download checksums are pinned in `machine.json`. Setup does not automatically upgrade an existing MSVC installation that satisfies the component requirements. This is repeatable provisioning, not a claim of bit-for-bit reproducible compiler output.

## Verification

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask test
```

Actual results are recorded in [verification.md](verification.md); platform and framework support that has not been exercised is stated there.

## Official installation references

- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
- [MSVC component IDs](https://learn.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-build-tools?view=vs-2022) and [installer commands](https://learn.microsoft.com/en-us/visualstudio/install/use-command-line-parameters-to-install-visual-studio?view=vs-2022)
- [Rust installation](https://rust-lang.org/tools/install/), [Node.js downloads](https://nodejs.org/en/download), [Go installation](https://go.dev/doc/install), [Git for Windows](https://git-scm.com/install/windows)
