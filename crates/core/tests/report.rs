//! A replay result counts the workflow steps that reached execution.

use build_machine_core::report::{Outcome, PlatformResult};

fn result(steps: serde_json::Value) -> PlatformResult {
    serde_json::from_value(serde_json::json!({
        "success": true, "status": "passed_with_limits", "finishedAt": "now", "attempts": 1, "log": "",
        "stages": { "setup": { "status": "passed", "startedAt": "now", "steps": steps } }
    }))
    .unwrap()
}

fn step(adapter: &str, status: &str, skipped: bool) -> serde_json::Value {
    serde_json::json!({ "index": 1, "name": adapter, "adapter": adapter, "status": status, "startedAt": "now", "skipped": skipped })
}

/// A step its `if` skipped, one a secret condition skipped and a skip marker
/// did not run; a failed step did.
#[test]
fn only_steps_that_reached_execution_count() {
    let mut none = result(serde_json::json!([
        step("run", "skipped", false),
        step("run", "passed_with_limits", true),
        step("skip", "passed_with_limits", false),
    ]));
    assert_eq!(none.executed_steps(), 0);
    none.require_executed_steps();
    assert!(!none.success);
    assert_eq!(none.status, Outcome::Failed);
    assert!(none.error.as_deref().unwrap_or_default().contains("실행된 workflow 단계가 없어요"));

    let mut ran = result(serde_json::json!([step("run", "skipped", false), step("checkout", "failed", false)]));
    assert_eq!(ran.executed_steps(), 1);
    ran.require_executed_steps();
    assert!(ran.success);
}
