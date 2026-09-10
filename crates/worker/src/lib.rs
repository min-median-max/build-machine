//! The native worker: everything the machine does on the computer it builds on.
//!
//! Exposed as a library so its behaviour can be tested directly, and driven by
//! the binary beside it.

pub mod build;
pub mod ci;
#[cfg(target_os = "linux")]
pub mod desktop;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod package;
pub mod provision;
pub mod run;
pub mod stream;
#[cfg(windows)]
pub mod win32;
pub mod workspace;

/// Markers that let the controller find the structured report in the worker's
/// output, so the report never has to be written to a read-only share.
pub const REPORT_BEGIN: &str = "BUILD_MACHINE_REPORT_BEGIN";
pub const REPORT_END: &str = "BUILD_MACHINE_REPORT_END";
