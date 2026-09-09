# Desktop control application

## Scope and acceptance

The initial desktop application is a macOS Tauri 2 app with a React interface and a Rust command bridge. Version is `0.0.1`. Rust runs the existing `build.py` controller; Python and Windows PowerShell remain the build workers. The application requires this checkout, Python, Git and Parallels. It does not yet provide a self-contained installer for another developer's machine.

The app discovers this checkout beside its build output or at `~/Work/build-machine`. Users can choose another controller folder with the native folder picker. The app lists recent projects from existing local build records and permits a different Git project folder. It remembers the selected controller, project and platforms locally.

1. Show each configured environment's actual VM connection state, separate from tool diagnosis. Each environment prominently retains the last diagnosis or setup result and its completion time across app restarts. Build and launch results do not replace that tool result. These are historical checks, not continuous monitoring. A changed machine configuration invalidates the displayed tool results until another check. Do not show an untested toolchain as ready.
2. Support selecting one or more of Windows, Ubuntu and macOS. All selected platforms in a build share one source snapshot.
3. Offer diagnosis, prerequisite setup, build, optional launch after build, and launch of the last build. Each action invokes the maintained CLI with argument arrays, without introducing a separate build recipe or shell interpolation.
4. Stream process output into a scrollable log and show per-platform results from the controller's structured result file. Preserve commands, process output and exit codes on disk so a failed step is identifiable. Missing tools, stopped VMs, invalid project paths, missing result files and nonzero exits appear as failures with their actual messages. Guest commands are not automatically repeated after an execution error: the controller must not start a second installer or build when completion is unknown.
5. Run one operation at a time. The existing controller lock still protects against concurrent terminal commands. Prevent normal app closure while an operation is running; the app does not claim to support safe cancellation of guest installers or builds.
6. Folder selection, platform selection, failure rendering and log streaming are tested in the browser with explicit mocked desktop APIs. The actual Rust command bridge is tested separately against controlled child processes and a real Ubuntu doctor invocation. Build and open the native macOS application for visual review. Record the exact scope of native UI automation rather than treating browser mocks as native execution evidence.
7. GUI actions must not reset app data, silently change machine tool versions, dispatch GitHub workflows or publish releases. The unfinished release command is not presented as a working GUI action.

## Development and records

The GUI source lives under `gui/`. `python3 desktop.py dev` starts the frontend development server and Tauri Rust development process together. `python3 desktop.py build --run` installs declared development tools as needed, installs locked GUI dependencies, builds the macOS app and opens it. The built app subsequently runs without entering a Python command, while still requiring Python internally for the controller.

The CLI accepts multiple names after `--os` and an optional `--result-file` for its machine-readable result. Existing single-platform commands remain valid. GUI preferences stay in the Tauri application configuration directory; operation logs and result files stay under the controller's ignored `.state/` directory.

The initial GUI, persistent environment results and actual setup controls are implemented. Native checks and the unresolved Parallels execution failure are recorded in [GUI-VERIFICATION.md](GUI-VERIFICATION.md). Three-OS release acceptance remains incomplete in [verification.md](verification.md).

References: [Tauri command bridge](https://v2.tauri.app/develop/calling-rust/), [Tauri channels](https://v2.tauri.app/develop/calling-frontend/), [native folder dialogs](https://v2.tauri.app/plugin/dialog/).
