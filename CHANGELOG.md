# Functional changes

## 0.0.1

- Add strict GitHub Actions workflow validation and local replay for supported `uses` and `run` steps. Record event/ref, source revision and dirty state, ordered setup/test/build/smoke/release stages, artifact checksums, unsigned limits and local external-service adapters.
- Add immutable ref snapshots, Git repository-root registration, default sequential matrix execution and explicit optional parallel execution with the same per-platform report fields. Bound persisted run reports with the machine retention policy.
- Add per-project workflow path, event/ref and matrix-mode settings to the desktop app and show replay metadata and limited results in the Dashboard.

- Add Dashboard with registered-project build summaries, recent logs and current GUI operation progress. Preserve partial reports and source-preparation failures so incomplete or failed builds do not retain an older success indication.

- Place shared environment diagnosis/setup behind the bottom Settings button. Select a registered project directly to open its build and launch controls. Add explicit `+` registration, duplicate-folder selection, registration removal and persistent per-project build targets/launch options. Preserve the existing selected project and expose the settings folder.

- Add a macOS Tauri 2 GUI for the maintained build commands, with platform selection, streamed output and native folder selection.
- Retain each environment's last diagnosis/setup result and completion time after app restart. Keep VM connection state and build results separate from historical tool checks.
- Preserve failed command details in the UI and logs, including required and installed Windows tool versions. Support multiple CLI platforms and structured result files.
- Document current project build requirements and unverified framework/release coverage.
