# Desktop GUI verification

[한국어](GUI-VERIFICATION.ko.md)

Verified on 2026-09-09, macOS ARM64 and Parallels Desktop 27.0.1 (58670). The implementation was tested in the working tree based on `4f7465e`; the initial GUI runtime-source SHA-256 was `3b851171f79eb117b25daeef2fe0fae96e16c9d725de735c698268d7e605eefc`. The built executable SHA-256 was `99e9ceeab09d2ccc2f8894bdb2303bea4038292c5e6f2416fabcdbffe27d5521`. The app is an unsigned ARM64 `0.0.1` bundle at `gui/src-tauri/target/release/bundle/macos/Build Machine.app`.

- Python controller/source/cache tests: 14 passed. Multi-OS builds use one snapshot, later environments still execute after a failure, tool results persist by environment and configuration, and Windows errors retain the actual diagnosis reason.
- Rust bridge tests: five passed; the separate real-Ubuntu doctor test also passed when explicitly invoked. The bridge preserves argument boundaries, stdout/stderr and failed exits; a missing result file cannot establish success.
- Browser tests: six passed using explicit mocked Tauri APIs. They cover folder/platform selection, launch selection, streamed logs, disabled controls, failure rendering, tool-result retention after reload/build, and failures without a result file. These tests do not establish native VM execution.
- `desktop.py build` produced the native app with locked frontend and Rust dependencies. `desktop.py dev` is the maintained combined frontend/Rust development entry point.
- Actual app controls ran Ubuntu doctor and setup. Setup at 20:07:44 KST succeeded and its record was preserved through an app restart. Logs: `20260909-200711-792265-matrix.log` and `20260909-200743-118295-matrix.log` under `.state/logs/`.
- Windows doctor correctly rejected Node.js 24.21.0 against the configured 22.23.2. The real GUI displayed `node: expected v22.23.2; found v24.21.0`; `.state/gui-native-windows-doctor.png` records that failure. Log: `20260909-201043-388895-matrix.log`.
- Actual Windows setup installed Node.js 22.23.2, reused other tools, and passed doctor with no missing requirements at 20:11:38. Log: `20260909-201120-666400-matrix.log`; screenshot after app restart: `.state/gui-native-windows-setup-reopened.png`.
- Actual multi-OS GUI setup passed at 20:13:29–30. Windows, Ubuntu and macOS reused all declared tools; Windows PATH was unchanged. All three completion labels and timestamps remained visible after reopening. Log: `20260909-201319-880035-matrix.log`; screenshot: `.state/gui-native-windows-linux-macos-setup-reopened.png`.

Native actions and window capture are maintained in `tests/native_gui.py` and `tests/gui_window.js`. AIRDATA sources, stored data and its running guest applications were preserved. GUI-driven application build/launch and package installation were not repeated during these setup checks; previous AIRDATA coverage remains in [verification.md](verification.md).

## Known Parallels execution failure

Ubuntu setup at 19:48:07 failed with exit 255 and `PrlJob_GetResult: Invalid argument`. The actual failed invocation was root `prlctl exec ... native.py setup-system`. The same action later succeeded without an execution-client fix.

`python3 tests/parallels_smoke.py --iterations 30` reproduced two failures among 60 direct `prlctl` invocations of harmless commands: one root `PrlJob_GetResult` failure and one current-user `PrlJob_GetRetCode` failure, both exit 255. The 60 `prlexec` invocations passed, but the installed `prlexec` script itself delegates to `prlctl`; this is not evidence of a repair or a separate reliable transport. The exact internal cause is unconfirmed. Native tool preparation currently succeeds, while intermittent execution-client failure remains unresolved.

The controller keeps these failures visible and records the command, output and exit code. It does not replay installers or builds automatically after an uncertain guest execution. No Parallels update, VM reset, private SDK replacement, GitHub workflow dispatch or release publication was performed.
