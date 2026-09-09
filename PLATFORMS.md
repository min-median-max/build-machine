# Release rehearsal across three operating systems

The Mac controls three native execution environments: this macOS host, a Windows VM in Parallels, and a Linux VM in Parallels. A Linux container on macOS does not stand in for Windows or macOS execution.

Each selected platform diagnoses its build tools, installs missing declared tools, builds the actual platform artifact, and checks the packaged application. Publication is represented by a local artifact manifest; release uploads and production signing require their own actual acceptance and are never reported as verified by a local mock.

Projects use one recorded source snapshot for the selected platforms. Source revision, working-tree changes, platform, CPU target, tool versions, command, result and artifact checksums are recorded. A cached development build can be reused, while a release rehearsal must execute packaging and smoke checks again. Dependency caches may be reused.

## Platform mapping

| Platform | Local execution | Intended GitHub Actions runner | Target |
| --- | --- | --- | --- |
| Windows | Windows 11 ARM64 in Parallels | `windows-11-arm` | `aarch64-pc-windows-msvc` |
| Linux | Ubuntu 26.04 ARM64 in Parallels | `ubuntu-26.04-arm` (public preview) | `aarch64-unknown-linux-gnu` |
| macOS | Current ARM64 Mac | `macos-14` for the existing airdata workflow | `universal-apple-darwin` |

Matching a build target does not make local OS images identical to GitHub-hosted images. Windows x64 and native Intel macOS execution are additional targets, not covered by ARM64 execution. A universal macOS artifact can contain both architectures while local launch verifies only the architecture actually executed.

## Current implementation status

Windows and Ubuntu provisioning, doctor, AIRDATA build and visible launch are implemented and verified. The common controller also includes the native macOS worker; macOS provisioning passed, while application and installer acceptance remain pending. Windows installer rehearsal and shared GitHub Actions integration are in progress. Ubuntu 26.04 LTS ARM64 is installed with Parallels Tools. Linux acceptance covers automatic dependency installation, a successful AIRDATA build, a visible application window, and repeated commands that reuse the verified build and process. See [verification.md](verification.md) for actual evidence.

The existing airdata workflow builds a macOS universal app with Node.js 22, pnpm 11 and stable Rust. Its release action publishes to GitHub and conditionally signs/notarizes. The Windows baseline initially used Node.js 24 and built an executable without an installer; that baseline alone does not validate the release workflow.

Reference: [GitHub-hosted runner labels and architectures](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
