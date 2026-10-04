//! A worker reads only a request written for its own protocol.

use build_machine_core::request::WorkRequest;

/// A request from a controller of another protocol is refused before the
/// worker does anything, naming both protocols and the rebuild.
#[test]
fn a_request_of_another_protocol_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("request.json");
    std::fs::write(
        &path,
        serde_json::json!({
            "protocol": "stale",
            "snapshot": {
                "revision": "r", "dirty": false, "sourceHash": "h", "fileCount": 0, "sourceMode": "local",
                "projectKey": "orm", "project": "/orm", "archive": ""
            },
            "archive": "",
            "target": "aarch64-apple-darwin",
        })
        .to_string(),
    )
    .unwrap();
    let error = WorkRequest::load(&path).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("stale") && message.contains("cargo xtask worker"), "{message}");
}
