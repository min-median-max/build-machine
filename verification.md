# Verification

The Windows build and launch baseline passed on 2026-09-09. Three-OS release rehearsal, installer verification, and GitHub Actions parity are not complete.

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

`airdata/.github/workflows/release-macos.yml` currently defines only macOS: `macos-14`, Node.js 22, pnpm 11, stable Rust, `universal-apple-darwin`, conditional Apple signing/notarization, and publication through `tauri-action`. The verified Windows baseline uses Node.js 24 and skips installer bundling. It does not reproduce that workflow, signing, publication, x64 Windows, Linux, or macOS acceptance. No GitHub workflow was dispatched and nothing was published.
