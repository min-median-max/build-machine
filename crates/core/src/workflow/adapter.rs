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
    GoSetup,
    PhpSetup,
    RustSetup,
    Cache,
    TauriBuild,
    ArtifactUpload,
    /// `actions/upload-pages-artifact`: the site a later deploy-pages step
    /// deploys.
    PagesArtifact,
    /// `actions/deploy-pages`: a recorded dry run that deploys nothing.
    DeployPages,
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
            Adapter::GoSetup => "go-setup",
            Adapter::PhpSetup => "php-setup",
            Adapter::RustSetup => "rust-setup",
            Adapter::Cache => "cache",
            Adapter::TauriBuild => "tauri-build",
            Adapter::ArtifactUpload => "artifact-upload",
            Adapter::PagesArtifact => "pages-artifact",
            Adapter::DeployPages => "deploy-pages",
            Adapter::Release => "release",
            Adapter::Run => "run",
            Adapter::Skip => "skip",
        }
    }

    /// Resolve an action name. `None` means this machine has no adapter for it,
    /// which fails validation rather than being silently ignored.
    ///
    /// GitHub resolves an action's owner and repository without regard to case,
    /// so `Swatinem/rust-cache` and `swatinem/rust-cache` are the same action.
    pub fn for_action(name: &str) -> Option<Adapter> {
        Some(match name.to_ascii_lowercase().as_str() {
            "actions/checkout" => Adapter::Checkout,
            "pnpm/action-setup" => Adapter::PnpmSetup,
            "actions/setup-node" => Adapter::NodeSetup,
            "actions/setup-go" => Adapter::GoSetup,
            "shivammathur/setup-php" => Adapter::PhpSetup,
            "dtolnay/rust-toolchain" => Adapter::RustSetup,
            "swatinem/rust-cache" | "actions/cache" => Adapter::Cache,
            "tauri-apps/tauri-action" => Adapter::TauriBuild,
            "actions/upload-artifact" => Adapter::ArtifactUpload,
            "actions/upload-pages-artifact" => Adapter::PagesArtifact,
            "actions/deploy-pages" => Adapter::DeployPages,
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
            | Adapter::GoSetup
            | Adapter::PhpSetup
            | Adapter::RustSetup
            | Adapter::Cache => "setup",
            Adapter::TauriBuild => "build",
            Adapter::ArtifactUpload | Adapter::PagesArtifact | Adapter::DeployPages | Adapter::Release => "release",
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
                | Adapter::GoSetup
                | Adapter::PhpSetup
                | Adapter::RustSetup
                | Adapter::Cache
        )
    }

    /// The `with` inputs this adapter honours, for the adapters that install
    /// what a workflow declares. An input outside this list changes what the
    /// action would do, so it fails validation instead of being ignored.
    /// `None` means the adapter does not read its inputs.
    pub fn inputs(&self) -> Option<&'static [&'static str]> {
        Some(match self {
            // `submodules`, `lfs`, `sparse-checkout` and `filter` would check
            // out something else.
            Adapter::Checkout => {
                &["fetch-depth", "fetch-tags", "clean", "persist-credentials", "repository", "ref", "path"]
            }
            Adapter::NodeSetup => &["node-version", "node-version-file", "cache", "cache-dependency-path"],
            Adapter::GoSetup => &["go-version", "go-version-file", "cache", "cache-dependency-path"],
            Adapter::PhpSetup => &["php-version", "php-version-file", "extensions", "tools", "coverage"],
            // `retention-days` bounds how long GitHub keeps the artifact,
            // which a replay does not keep past its run.
            Adapter::PagesArtifact => &["path", "name", "retention-days"],
            // `preview`, `token` and the timing inputs would deploy elsewhere
            // or wait on GitHub's deployment.
            Adapter::DeployPages => &["artifact_name"],
            _ => return None,
        })
    }

    /// Adapters that replace an external GitHub service and must be recorded as
    /// a limit rather than reported as a completed publication.
    pub fn is_external_service(&self) -> bool {
        matches!(self, Adapter::ArtifactUpload | Adapter::PagesArtifact | Adapter::DeployPages | Adapter::Release)
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
