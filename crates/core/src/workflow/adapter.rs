//! Which GitHub actions this machine can stand in for, and what each one is.

/// The local stand-in for a supported action.
///
/// This travels from the controller to the worker, so it is serialized as the
/// name it is known by rather than being reconstructed from a second field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Adapter {
    Checkout,
    PnpmSetup,
    NodeSetup,
    RustSetup,
    Cache,
    TauriBuild,
    ArtifactUpload,
    Release,
    /// A shell `run` step.
    #[default]
    Run,
    /// A stage the workflow documents as deliberately absent.
    Skip,
}

impl Adapter {
    pub fn as_str(&self) -> &'static str {
        match self {
            Adapter::Checkout => "checkout",
            Adapter::PnpmSetup => "pnpm-setup",
            Adapter::NodeSetup => "node-setup",
            Adapter::RustSetup => "rust-setup",
            Adapter::Cache => "cache",
            Adapter::TauriBuild => "tauri-build",
            Adapter::ArtifactUpload => "artifact-upload",
            Adapter::Release => "release",
            Adapter::Run => "run",
            Adapter::Skip => "skip",
        }
    }

    /// Resolve an action name. `None` means this machine has no adapter for it,
    /// which fails validation rather than being silently ignored.
    pub fn for_action(name: &str) -> Option<Adapter> {
        Some(match name {
            "actions/checkout" => Adapter::Checkout,
            "pnpm/action-setup" => Adapter::PnpmSetup,
            "actions/setup-node" => Adapter::NodeSetup,
            "dtolnay/rust-toolchain" => Adapter::RustSetup,
            "swatinem/rust-cache" | "actions/cache" => Adapter::Cache,
            "tauri-apps/tauri-action" => Adapter::TauriBuild,
            "actions/upload-artifact" | "actions/upload-pages-artifact" => Adapter::ArtifactUpload,
            "softprops/action-gh-release" => Adapter::Release,
            _ => return None,
        })
    }

    /// The stage an action belongs to.
    ///
    /// An action's own name must never decide this. `actions/checkout` contains
    /// "check", and reading it as a test step let a workflow with no test
    /// command satisfy the test gate without the required skip comment.
    pub fn stage(&self) -> Option<&'static str> {
        Some(match self {
            Adapter::Checkout
            | Adapter::PnpmSetup
            | Adapter::NodeSetup
            | Adapter::RustSetup
            | Adapter::Cache => "setup",
            Adapter::TauriBuild => "build",
            Adapter::ArtifactUpload | Adapter::Release => "release",
            // A shell step is classified by its own name and command; a skip
            // marker is placed directly into the stage it stands for.
            Adapter::Run | Adapter::Skip => return None,
        })
    }

    /// Whether the local stand-in is a real execution or a recorded limit.
    pub fn is_local_stand_in(&self) -> bool {
        matches!(
            self,
            Adapter::Checkout
                | Adapter::PnpmSetup
                | Adapter::NodeSetup
                | Adapter::RustSetup
                | Adapter::Cache
        )
    }

    /// Adapters that replace an external GitHub service and must be recorded as
    /// a limit rather than reported as a completed publication.
    pub fn is_external_service(&self) -> bool {
        matches!(self, Adapter::ArtifactUpload | Adapter::Release)
    }
}

/// Classify a shell step from its name and command.
pub fn stage_for_shell(name: &str, run: &str) -> &'static str {
    let text = format!("{name} {run}").to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| text.contains(word));
    if has(&["smoke", "launch", "health", "e2e"]) {
        "smoke"
    } else if has(&["test", "lint", "check", "verify"]) {
        "test"
    } else if has(&["build", "package", "compile"]) {
        "build"
    } else {
        "setup"
    }
}

pub const STAGE_ORDER: [&str; 5] = ["setup", "test", "build", "smoke", "release"];
