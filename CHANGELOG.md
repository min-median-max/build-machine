# Functional changes

## 0.0.1

- Place shared environment diagnosis/setup behind the bottom Settings button. Select a registered project directly to open its build and launch controls. Add explicit `+` registration, duplicate-folder selection, registration removal and persistent per-project build targets/launch options. Preserve the existing selected project and expose the settings folder.

- Add a macOS Tauri 2 GUI for the maintained build commands, with platform selection, streamed output and native folder selection.
- Retain each environment's last diagnosis/setup result and completion time after app restart. Keep VM connection state and build results separate from historical tool checks.
- Preserve failed command details in the UI and logs, including required and installed Windows tool versions. Support multiple CLI platforms and structured result files.
- Document current project build requirements and unverified framework/release coverage.
