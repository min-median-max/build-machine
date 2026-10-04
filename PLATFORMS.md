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

## Disk space of the virtual machines

A Parallels disk (`type='expanded'`) grows as the guest writes and keeps the blocks it holds after the guest deletes files. It gives them back only when two things happen together: the guest discards the blocks its file system no longer uses, and Parallels compacts the image online, punching those blocks out of the `.hds` file on the Mac. The Linux guest's SATA disk accepts discards (`lsblk --discard` shows a 4K granularity).

The controller turns on `online-compact` for every disk of the Linux VM that lacks it before setup, build, release and replay, and afterwards — whether the work succeeded or not — runs the worker's elevated `reclaim`, which is `fstrim --all` in the guest. Measure the result by the blocks the `.hds` file allocates (`du -k`), not by its apparent size (`ls`, Finder), which stays at its largest: on 2026-10-04 the image stayed at 42,838,523,904 bytes apparent while its allocation fell from 40 GB to 26,993,664 KiB after `fstrim` trimmed 37.6 GiB in the running guest.

Online compaction after the guest's TRIM is not reliable: of three measured trims on 2026-10-04 (4 GiB, 8.8 GiB, 8.9 GiB), one gave the space back within 30 seconds and two left the image unchanged for 10 and 15 minutes. Compaction with the VM stopped does give it back once the guest has trimmed: `prlctl stop`, then `prl_disk_tool compact --hdd "<vm>.pvm/harddisk1.hdd"`, took 7 seconds and shrank the image from 35,398,656 KiB to 27,033,600 KiB, its apparent size from 36.2 GB to 27.7 GB.

So after setup, build, release and replay on Linux, whatever their result, the controller runs the guest's `reclaim`, stops the VM, runs `prl_disk_tool compact --hdd` on each of its disks, starts it, waits until the guest runs a command (each attempt logged, at most 300 seconds), and runs `setup-system` and `doctor` as the next work would. It holds the machine lock throughout, as every front end does for a whole operation. The platform result's `disk` records each image's allocated and apparent bytes before and after; a VM that is not ready again fails the platform with that cause.

## Current implementation status

The macOS [desktop controller](GUI.md) and the `build-machine` command are two front ends over one controller library. Its tool results persist separately from VM connection state. Current supported project layouts and framework gaps are defined in [PROJECTS.md](PROJECTS.md). Parallels command stability remains a known issue recorded in [verification.md](verification.md).

Windows and Ubuntu provisioning, doctor, AIRDATA build and visible launch are implemented and verified. The common controller also includes the native macOS worker; macOS provisioning passed, while application and installer acceptance remain pending. Windows installer rehearsal and shared GitHub Actions integration are in progress. Ubuntu 26.04 LTS ARM64 is installed with Parallels Tools. Linux acceptance covers automatic dependency installation, a successful AIRDATA build, a visible application window, and repeated commands that reuse the verified build and process. See [verification.md](verification.md) for actual evidence.

The existing airdata workflow builds a macOS universal app with Node.js 22, pnpm 11 and stable Rust. Its release action publishes to GitHub and conditionally signs/notarizes. The Windows baseline initially used Node.js 24 and built an executable without an installer; that baseline alone does not validate the release workflow.

Reference: [GitHub-hosted runner labels and architectures](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
