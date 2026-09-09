# Build machine

[한국어](README.ko.md)

This repository provides inspectable build-machine commands that run without Codex. The [three-OS release rehearsal specification](PLATFORMS.md) defines the shared objective. Windows and Ubuntu provisioning, diagnosis, AIRDATA builds and visible launches have been exercised. macOS provisioning has been exercised; its application and installer acceptance remain pending. Each operating system shares its declared tools across projects and stores sources, outputs and logs in separate project directories.

## Required behavior

- `setup` checks the declared versions and required MSVC components, installs missing tools, and preserves existing installations and application data. Running it again does not reinstall tools that satisfy the configuration or add duplicate PATH entries.
- `doctor` reports the actual selected environment and missing requirements.
- `build` runs the same prerequisite checks and installation procedure automatically. A separate remembered setup step is not required.
- `build PROJECT` copies current Git tracked files and nonignored untracked files, including uncommitted edits. It excludes ignored files and rejects symlinks outside the project. It records the Git revision, source checksum, tool versions, build command, log path and executable checksum.
- Source snapshots are stored separately by content. Builds occur on the selected operating system's local disk. A successful result is reused only when its source, recipe, worker code, machine configuration and executable checksum match. The native worker preserves executable permissions on source scripts.
- `run PROJECT` starts the last successful executable in the signed-in desktop. Repeating it reuses that executable's running process. A process/window check verifies launch; it does not certify all application features.
- Standard Tauri 2 projects use their locked dependencies and produce a release executable. Windows Wails 2 projects use the CLI version declared by their Go module. Other layouts and native Wails projects can supply an explicit build command and executable path.
- Commands and logs are inspectable files. No credentials, conversational memory or assistant-specific service is required.

## Commands

The [Tauri 2 desktop app](GUI.md) opens with a dashboard of registered projects, their latest recorded build results and recent build logs. It shows the current GUI operation while allowing navigation back to that operation. Use `+` to register a Git folder, then select its entry to configure build targets, build and launch. Each project retains its own options. The bottom **Settings** button opens Windows/Ubuntu/macOS diagnosis and automatic tool setup shared by all projects. Each environment retains its last diagnosis/setup result and completion time across app restarts. Connection state and historical tool checks are displayed separately. [Project build requirements](PROJECTS.md) describe the supported layouts and remaining framework coverage.

Registration and GUI settings live in `~/Library/Application Support/local.buildmachine.desktop/preferences.json`. Use **Settings → Open settings folder** to inspect that file. Project registrations are independent of recent build logs. Removing a registration leaves its source folder and app data intact.

```sh
cd ~/Work/build-machine
python3 desktop.py build --run
```

After building, open `gui/src-tauri/target/release/bundle/macos/Build Machine.app` directly. This initial macOS app is unsigned and still needs this checkout and Python internally. `python3 desktop.py dev` starts React and Rust/Tauri development together; `python3 desktop.py test` verifies the frontend build, Rust bridge and browser interactions. The GUI does not expose unfinished release publication.

Requirements on the Mac: Python 3, Git, and Parallels with working `prlctl exec` and Parallels Tools. The selected VM must be running with a desktop user signed in. Linux requires Python 3.10 or later; Ubuntu 26.04 supplies Python 3.14. The VM names and tool versions are in [machine.json](machine.json).

The common controller selects `windows`, `linux`, `macos` or `all`. Supply multiple names with `--os windows linux`; omitting `--os` selects all three. A multi-platform build captures one source snapshot, attempts each selected platform, records each result and fails overall if any platform fails. `--result-file PATH` writes the structured result for other interfaces. Diagnosis/setup results and timestamps persist in `.state/tool-status.json`; changed machine configuration invalidates those displayed results.

```sh
cd ~/Work/build-machine
python3 build.py doctor --os linux
python3 build.py setup --os linux
python3 build.py build ~/Work/airdata --os linux --run
python3 build.py run ~/Work/airdata --os linux
```

`build` installs missing prerequisites automatically. A separate `setup` is optional. Linux system packages are installed as root through Parallels, while builds and launches run as the signed-in desktop user. The launch command reads only the desktop connection settings from that user's systemd environment. It checks that the process remains running. A separate screen capture verifies visible rendering.

The original Windows-only entry point remains available:

```sh
cd ~/Work/build-machine
python3 winbuild.py setup
python3 winbuild.py doctor
python3 winbuild.py build ~/Work/airdata --run
python3 winbuild.py run ~/Work/airdata
```

The same commands accept another project directory. `winbuild.py --vm NAME` selects a different Windows VM; the common controller uses `machine.json`. Custom projects supply `--framework custom --command 'BUILD COMMAND' --artifact 'RELATIVE/PATH'` to `build`. Custom commands run in the selected platform's shell as the desktop user inside that project's snapshot.

The machine uses a read-only Parallels share named `WindowsBuildMachine` for transfer. It copies its control scripts to Windows, so they can also be inspected and invoked in Windows PowerShell. Local transfer files, execution records and logs are under `.state/` and excluded from Git. Windows projects and their build receipts are under `C:\BuildMachine\projects`. MSVC is installed at `C:\BuildTools`; user tools are under `%LOCALAPPDATA%\WindowsBuildMachine`.

Linux reads the same share at `/media/psf/WindowsBuildMachine`. Linux and macOS keep managed user tools under `~/.local/share/build-machine` and build receipts under `~/.local/state/build-machine/PROJECT_KEY/latest.json`. Native setup preserves the system's default Node.js and Go installations. Ubuntu Tauri builds produce an ARM64 executable and a `.deb`; creating the package does not verify a system package installation. The `release` path is unfinished across all three platforms and is not a completed GitHub Actions release rehearsal.

MSVC uses Microsoft's serviced Visual Studio 2022 channel and verifies required components. Other tool versions and download checksums are pinned in `machine.json`. Setup does not automatically upgrade an existing MSVC installation that satisfies the component requirements. This is repeatable provisioning, not a claim of bit-for-bit reproducible compiler output.

## Verification

Native GUI checks use `python3 tests/native_gui.py doctor --os windows linux macos` or `python3 tests/native_gui.py setup --os linux --restart`. They press actual macOS app controls, verify the real result, and capture the app window. They require macOS Accessibility and screen-capture access for the invoking application. Setup can install declared missing prerequisites. Inspect the capture to verify visible results after reopening; browser mocks alone do not establish native behavior.

Parallels 27.0.1 has produced intermittent `prlctl exec` errors during local checks. `python3 tests/parallels_smoke.py --transport prlctl --iterations 30` runs harmless guest commands and checks identity, output and exit status. This reproducible probe does not repair the execution client. Failed guest operations are reported without automatic replay; see [verification.md](verification.md) for the observed failures and limits.

Run `python3 -m unittest discover -s tests` on macOS or Linux. Live acceptance requires successful repeated provisioning, an actual build and visible application launch, and repeated build/run commands that reuse the verified result and running process. Actual results are recorded in [verification.md](verification.md); platform and framework support that has not been exercised is stated there.

## Official installation references

- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
- [MSVC component IDs](https://learn.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-build-tools?view=vs-2022) and [installer commands](https://learn.microsoft.com/en-us/visualstudio/install/use-command-line-parameters-to-install-visual-studio?view=vs-2022)
- [Rust installation](https://rust-lang.org/tools/install/), [Node.js downloads](https://nodejs.org/en/download), [Go installation](https://go.dev/doc/install), [Git for Windows](https://git-scm.com/install/windows)
- [Wails 2 installation](https://wails.io/docs/gettingstarted/installation/)
