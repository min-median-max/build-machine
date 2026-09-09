# Windows build machine

[한국어](README.ko.md)

This repository defines a Windows ARM64 development environment in Parallels. Its commands work from a normal macOS terminal without Codex. Node.js, pnpm, Rust, Go, Git, MSVC and the Windows SDK are shared by projects; each project's sources, build outputs and logs have their own directory on the Windows disk.

## Required behavior

- `setup` checks the declared versions and required MSVC components, installs missing tools, and preserves existing installations and application data. Running it again does not reinstall tools that satisfy the configuration or add duplicate PATH entries.
- `doctor` reports the actual Windows environment and missing requirements.
- `build` runs the same prerequisite checks and installation procedure automatically. A separate remembered setup step is not required.
- `build PROJECT` copies current Git tracked files and nonignored untracked files, including uncommitted edits. It excludes ignored files and rejects symlinks outside the project. It records the Git revision, source checksum, tool versions, build command, log path and executable checksum.
- Source snapshots are stored separately by content. Builds occur on the Windows disk. A successful result is reused only when its source, recipe, machine configuration and executable checksum match.
- `run PROJECT` starts the last successful executable in the signed-in Windows desktop. Repeating it reuses that executable's running process. A process/window check verifies launch; it does not certify all application features.
- Standard Tauri 2 projects use their locked dependencies and produce a release executable. Wails 2 projects use the CLI version declared by their Go module. Other layouts can supply an explicit Windows build command and executable path.
- Commands and logs are inspectable files. No credentials, conversational memory or assistant-specific service is required.

## Commands

Requirements on the Mac: Python 3, Git, and Parallels with working `prlctl exec` and Parallels Tools. Windows must be running with a desktop user signed in. The VM name and tool versions are in [machine.json](machine.json).

```sh
cd ~/Work/windows-build-machine
python3 winbuild.py setup
python3 winbuild.py doctor
python3 winbuild.py build ~/Work/airdata --run
python3 winbuild.py run ~/Work/airdata
```

The same commands accept another project directory. `--vm NAME` selects a different VM. Custom projects supply `--framework custom --command 'WINDOWS BUILD COMMAND' --artifact 'RELATIVE/PATH.exe'` to `build`. Custom commands are executed as the desktop user inside that project's snapshot.

The machine uses a read-only Parallels share named `WindowsBuildMachine` for transfer. It copies its control scripts to Windows, so they can also be inspected and invoked in Windows PowerShell. Local transfer files, execution records and logs are under `.state/` and excluded from Git. Windows projects and their build receipts are under `C:\BuildMachine\projects`. MSVC is installed at `C:\BuildTools`; user tools are under `%LOCALAPPDATA%\WindowsBuildMachine`.

MSVC uses Microsoft's serviced Visual Studio 2022 channel and verifies required components. Other tool versions and download checksums are pinned in `machine.json`. Setup does not automatically upgrade an existing MSVC installation that satisfies the component requirements. This is repeatable provisioning, not a claim of bit-for-bit reproducible compiler output.

## Verification

Run `python3 -m unittest discover -s tests`. Live acceptance requires two successful setup runs, an actual Windows build and visible application launch, and repeated build/run commands that reuse the verified result and running process. Actual results are recorded in [verification.md](verification.md); framework support that has not been exercised is stated there.

## Official installation references

- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
- [MSVC component IDs](https://learn.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-build-tools?view=vs-2022) and [installer commands](https://learn.microsoft.com/en-us/visualstudio/install/use-command-line-parameters-to-install-visual-studio?view=vs-2022)
- [Rust installation](https://rust-lang.org/tools/install/), [Node.js downloads](https://nodejs.org/en/download), [Go installation](https://go.dev/doc/install), [Git for Windows](https://git-scm.com/install/windows)
- [Wails 2 installation](https://wails.io/docs/gettingstarted/installation/)
