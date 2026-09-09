# Verification

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
