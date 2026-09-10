# Project build requirements

[한국어](PROJECTS.ko.md)

These are the build machine's current project requirements. A project is registered only at the Git repository root. Tauri and Wails do not require a separate build-machine manifest: the repository workflow is the CI contract when workflow replay is selected.

## Current automatic recipes

| Project | Required files and behavior | Current coverage |
| --- | --- | --- |
| Tauri 2 | `src-tauri/tauri.conf.json`, `src-tauri/Cargo.lock`, a root `pnpm-lock.yaml` or `package-lock.json`, and the project Tauri CLI dependency. The configured frontend build must work without prompts. | Windows ARM64 executable and Ubuntu ARM64 executable/DEB exercised with AIRDATA. The macOS universal worker is implemented but AIRDATA acceptance is pending. |
| Wails 2 | Root `wails.json` for detection. | No automatic recipe. Supply an explicit command through the custom recipe. |
| Wails 3 beta | No automatic recipe or detection yet. | Requires explicit project commands through the CLI. Not verified. |
| Other layouts | An explicit build command and executable path relative to the source snapshot. | CLI custom recipe exists. Not yet exercised with a real project. Custom macOS app launch is not implemented. |

The GUI currently uses automatic detection. Custom commands and artifact paths are CLI options:

```sh
build-machine build /path/to/project --os linux \
  --framework custom --command './scripts/build-linux.sh' \
  --artifact 'build/bin/example'
```

Custom commands run in `cmd.exe /d /s /c` on Windows and `/bin/sh -eu -c` on Linux/macOS. Supply a separate platform command when those shells or output paths differ.

## Conditions shared by projects

- Use a Git checkout with an existing commit. Include build scripts and dependency lockfiles in Git. The source snapshot also includes current edits and nonignored untracked files; ignored machine-local dependencies and secrets are not copied. External symlinks and Git submodules are unsupported by the snapshotter.
- Keep the build independent of an individual developer's absolute paths, interactive prompts and local development server. Use the framework's production frontend build and resolve project files from the checkout.
- Make the declared compiler/runtime versions compatible with the project. Tools are currently pinned in the machine's `machine.json`, not selected independently from each project's version files. Native ARM64 dependencies are required for Windows/Linux; the macOS worker requests both ARM64 and Intel code in a universal app.
- Compile platform-specific code for the intended OS. Account for Windows WebView2 and Linux WebKit/system libraries. The machine provisions its declared prerequisites; it does not infer arbitrary native libraries from source code.
- Produce one identifiable executable, or specify `--artifact`. Tauri macOS builds must produce exactly one `.app`. Successful compilation, package creation, package installation, visible launch and application feature checks are separate results.
- Keep release signing and publication settings separate from local rehearsal. The current workers do not verify production signing, notarization or release publication. An unsigned local success does not establish GitHub Actions release parity.

`doctor` checks machine prerequisites and declared versions. `ci validate` checks the selected workflow's project contract; it does not claim application feature behavior, production signing, notarization or GitHub publication. A workflow can therefore pass with explicit `passed_with_limits` status while those external services remain unverified.

## Workflow replay contract

`ci validate` and `ci run` read one `.github/workflows/*.yml` file. The supported subset includes jobs with `runs-on`, `needs`, `if`, `env`, `with`, `working-directory`, `run`, and the adapters listed below. Jobs with containers, services, reusable workflows or matrix strategies fail validation instead of being silently changed.

Supported action adapters are `actions/checkout`, `pnpm/action-setup`, `actions/setup-node`, `dtolnay/rust-toolchain`, `swatinem/rust-cache`, `actions/cache`, `tauri-apps/tauri-action`, `actions/upload-artifact`, `actions/upload-pages-artifact`, and `softprops/action-gh-release`. Shell `run` steps execute in the selected worker. Checkout, cache, artifact upload, release, signing and GitHub status operations use local adapters; the report marks those limits and never treats them as production publication.

A step that uses a supported action is assigned its stage by that adapter: checkout, pnpm/node/Rust setup and caches are `setup`, the Tauri action is `build`, artifact upload and release are `release`. Only shell `run` steps are classified by keywords in their name and command, so an action's own name can never satisfy a stage gate. The default stage order is `setup`, `test`, `build`, `smoke`, then `release`. A workflow with no test or smoke command must include an exact comment such as `# build-machine: skip smoke reason=desktop smoke is verified separately`. The reason is stored in the platform result. Unknown actions, expressions, event/ref mismatches and missing gates fail validation.

The source is either the current Git working tree, including nonignored edits, or an immutable `--ref` archive. Every matrix entry records the same source revision, source hash and dirty state. Sequential execution is the default; `--execution parallel` runs selected operating systems concurrently while retaining identical per-stage fields and deterministic dashboard ordering.

For this GUI, `python3 desktop.py dev` starts the React development server and Rust/Tauri process together. Other projects retain their own Tauri or Wails development commands and configuration. No fixed frontend port is required by the build machine.

References: [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), [Tauri development](https://v2.tauri.app/develop/), [Wails 2 installation](https://wails.io/docs/gettingstarted/installation/), [Wails 3 documentation](https://v3.wails.io/).
