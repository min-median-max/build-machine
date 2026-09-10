# Functional changes

## 0.1.0

Rewritten as one Rust workspace. Python and PowerShell are removed; no script remains.

- The controller is a library. The command line and the desktop application are two front ends over it, so a job runs in the application's own process instead of through a spawned command and a result file. The `--result-file` handshake, its stdout parsing and its "the result file is missing" failure path are gone.
- The run report, the machine definition, the source snapshot and the work request are defined once and shared. The desktop bridge deserializes the report into that type instead of indexing an untyped document, so a renamed field is a compile error rather than a silently blank dashboard.
- Each operating system runs a prebuilt worker binary. The guest needs no Python, no PowerShell and no Rust toolchain. `.github/workflows/release.yml` builds each worker natively on its own runner — nothing is cross-linked — and assembles the macOS application around them.
- The workflow reader uses a YAML parser with typed deserialization. Failing closed on an unsupported construct is now a property of the types rather than of a hand-written parser.

### Behaviour that changed with the rewrite

The two workers had drifted apart. A single implementation settles each difference:

- A workflow replay returns its report on the worker's own output. The Windows worker previously tried to write that report onto the share, which is mounted read only, so a Windows replay could not have completed.
- One status rule. Windows could report a plain `passed` for a replay while the Unix worker never could.
- A replay records its artifacts with checksums on every platform. Windows previously left the list empty.
- Step timeouts apply on every platform, and a timeout kills the whole process group so a shell's own children cannot hold the pipe open past it.
- Checksums are lower case and written documents carry no byte order mark, on every platform.
- Source snapshot hashes change with the new archive writer, so the first build after this version rebuilds rather than reusing an earlier result.
- There is no migration from the previous `.state` layout, and `winbuild.py`'s separate Windows entry point is not restored. `build-machine` is the only command.

## 0.0.1

### Fixed

- `setup` no longer aborts the whole matrix on Windows. That branch left its platform result unassigned, raising UnboundLocalError past every handler; because `--os` defaults to all three environments and Windows runs first, plain `python3 build.py setup` and the desktop app's shared tool preparation both failed outright. An unexpected defect in any single platform is now recorded as that platform's failure, with its type, instead of ending the run without a report.
- Run retention discarded the newest runs first when over its byte cap, and measured only the report files while the logs that hold the actual bytes were never bounded at all. A run is now retained and evicted as one unit — its report and its logs together — with age, per-project count and total size applied independently and the oldest runs discarded first. An abandoned `running` report is reclaimed by the age policy; it previously survived forever and permanently masked its project's last real result. Logs that no retained run points at, including those of the `winbuild.py` entry point, are bounded by age, and the `--result-file` copies the desktop app requests are cleaned up alongside them.
- Every run is now recorded once, at `.state/runs/<run>/report.json`. The dashboard previously read `.state/<run>-result.json`, which no policy ever cleaned, while the retention policy operated on a separate copy nothing read. The unread `manifest.json` copy is gone.
- `actions/checkout` no longer satisfies the test gate. Steps that use a supported action are classified by their adapter; only shell `run` steps are matched on keywords. The action's name contains `check`, so a workflow with no test command passed validation without the required `# build-machine: skip test reason=...` comment. **Workflows that relied on this now fail validation until they add that comment or a real test step.**
- Workflow replays appear in the dashboard. A project with a workflow path runs `ci run`, and every such record was filtered out of the history it was documented to appear in. Replay rows are labelled, and the parsed workflow and step definitions are no longer copied into the dashboard payload.
- The operation log a run points at is no longer empty. It was written only on failure, so the dashboard's log button opened a file that did not exist for any successful run. It now records the run, each platform's outcome and where that platform's command output was written.
- Workflow step output is streamed instead of buffered until the step exits, both for `run` steps and for the Tauri action, and reaches the desktop app during a replay rather than only after it.
- Each stage's steps, with their command, exit code and captured output, are shown per platform in the dashboard. The runner already recorded them and nothing displayed them.

### Added

- Add strict GitHub Actions workflow validation and local replay for supported `uses` and `run` steps. Record event/ref, source revision and dirty state, ordered setup/test/build/smoke/release stages, artifact checksums, unsigned limits and local external-service adapters.
- Add immutable ref snapshots, Git repository-root registration, default sequential matrix execution and explicit optional parallel execution with the same per-platform report fields. Bound persisted run reports with the machine retention policy.
- Add per-project workflow path, event/ref and matrix-mode settings to the desktop app and show replay metadata and limited results in the Dashboard.

- Add Dashboard with registered-project build summaries, recent logs and current GUI operation progress. Preserve partial reports and source-preparation failures so incomplete or failed builds do not retain an older success indication.

- Place shared environment diagnosis/setup behind the bottom Settings button. Select a registered project directly to open its build and launch controls. Add explicit `+` registration, duplicate-folder selection, registration removal and persistent per-project build targets/launch options. Preserve the existing selected project and expose the settings folder.

- Add a macOS Tauri 2 GUI for the maintained build commands, with platform selection, streamed output and native folder selection.
- Retain each environment's last diagnosis/setup result and completion time after app restart. Keep VM connection state and build results separate from historical tool checks.
- Preserve failed command details in the UI and logs, including required and installed Windows tool versions. Support multiple CLI platforms and structured result files.
- Document current project build requirements and unverified framework/release coverage.
