//! The build machine command line.
//!
//! One entry point for every environment. It is a thin front end over the
//! controller library the desktop application also uses, so both drive exactly
//! the same code.

use anyhow::{bail, Result};
use build_machine_controller::{
    controller_root, find_controller, matrix, printing_observer, snapshot, state_directory, Lock, Operation,
};
use build_machine_core::config::Machine;
use build_machine_core::report::{Action, ExecutionMode};
use build_machine_core::request::Framework;
use build_machine_core::Platform;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "build-machine", about = "Diagnose, provision and build native projects from this Mac")]
struct Cli {
    /// The controller folder holding machine.json. Defaults to this executable's.
    #[arg(long, global = true)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Clone)]
struct Selection {
    /// Which environments to use. Omitted means all three.
    #[arg(long = "os", num_args = 1.., value_parser = parse_platform)]
    platforms: Vec<Platform>,
    /// Run the selected environments one after another, or at the same time.
    #[arg(long, default_value = "sequential", value_parser = parse_execution)]
    execution: ExecutionMode,
    /// Also write the structured result here, for another interface to read.
    #[arg(long)]
    result_file: Option<PathBuf>,
}

#[derive(Args, Clone)]
struct Recipe {
    #[arg(long, value_parser = parse_framework)]
    framework: Option<Framework>,
    /// An explicit command, run in the selected platform's shell.
    #[arg(long)]
    command: Option<String>,
    /// The executable path relative to the source snapshot.
    #[arg(long)]
    artifact: Option<String>,
    /// Launch the result once it is built.
    #[arg(long)]
    run: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Report the actual selected environment and missing requirements.
    Doctor {
        #[command(flatten)]
        selection: Selection,
    },
    /// Check declared versions and install what is missing.
    Setup {
        #[command(flatten)]
        selection: Selection,
    },
    /// Build a project.
    Build {
        project: PathBuf,
        #[command(flatten)]
        selection: Selection,
        #[command(flatten)]
        recipe: Recipe,
    },
    /// Build and check the installable package.
    Release {
        project: PathBuf,
        #[command(flatten)]
        selection: Selection,
        #[command(flatten)]
        recipe: Recipe,
    },
    /// Launch the last successful build.
    Run {
        project: PathBuf,
        #[command(flatten)]
        selection: Selection,
    },
    /// Validate or replay a repository GitHub Actions workflow.
    Ci {
        #[command(subcommand)]
        action: CiCommand,
    },
}

#[derive(Args, Clone)]
struct Replay {
    project: PathBuf,
    /// The workflow path relative to the repository root.
    #[arg(long)]
    workflow: Option<String>,
    #[arg(long, default_value = "workflow_dispatch")]
    event: String,
    /// A commit, branch or tag to replay. Omitted means the current tree.
    #[arg(long = "ref")]
    reference: Option<String>,
    #[command(flatten)]
    selection: Selection,
}

#[derive(Subcommand)]
enum CiCommand {
    /// Check the workflow contract without running it.
    Validate {
        #[command(flatten)]
        replay: Replay,
    },
    /// Run the supported workflow steps in the selected environments.
    Run {
        #[command(flatten)]
        replay: Replay,
    },
}

fn parse_platform(value: &str) -> Result<Platform, String> {
    value.parse().map_err(|error: anyhow::Error| error.to_string())
}

fn parse_execution(value: &str) -> Result<ExecutionMode, String> {
    match value {
        "sequential" => Ok(ExecutionMode::Sequential),
        "parallel" => Ok(ExecutionMode::Parallel),
        other => Err(format!("Unknown execution mode: {other}")),
    }
}

fn parse_framework(value: &str) -> Result<Framework, String> {
    value.parse().map_err(|error: anyhow::Error| error.to_string())
}

fn platforms(selection: &Selection) -> Vec<Platform> {
    if selection.platforms.is_empty() {
        Platform::ALL.to_vec()
    } else {
        let mut chosen = selection.platforms.clone();
        chosen.sort();
        chosen.dedup();
        chosen
    }
}

fn main() {
    if let Err(error) = execute() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}

fn execute() -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("Run this controller on macOS; the worker binary is what runs on each target.");
    }
    let cli = Cli::parse();
    let root = match &cli.root {
        Some(root) => controller_root(root)?,
        None => controller_root(&find_controller().unwrap_or_else(|| PathBuf::from(".")))?,
    };
    let machine = Machine::load(&root.join("machine.json"))?;

    // Validation reads the repository and reports; it starts no work, so it
    // does not take the lock or record a run.
    if let Command::Ci { action: CiCommand::Validate { replay } } = &cli.command {
        let operation = build_operation(&root, &machine, Action::Ci, replay.selection.clone(), Some(replay.clone()), None);
        let state = state_directory(&root);
        let prepared = snapshot::for_replay(&operation, &state)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "valid",
                "project": prepared.snapshot.project,
                "workflow": prepared.snapshot.workflow_path,
                "event": prepared.snapshot.event,
                "ref": prepared.snapshot.requested_ref,
                "revision": prepared.snapshot.revision,
                "dirty": prepared.snapshot.dirty,
                "stages": prepared.snapshot.stage_counts,
                "platforms": build_machine_core::workflow::declared_platforms(&prepared.workflow),
            }))?
        );
        return Ok(());
    }

    let (action, selection, recipe, replay, project) = match cli.command {
        Command::Doctor { selection } => (Action::Doctor, selection, None, None, None),
        Command::Setup { selection } => (Action::Setup, selection, None, None, None),
        Command::Build { project, selection, recipe } => (Action::Build, selection, Some(recipe), None, Some(project)),
        Command::Release { project, selection, recipe } => {
            (Action::Release, selection, Some(recipe), None, Some(project))
        }
        Command::Run { project, selection } => (Action::Run, selection, None, None, Some(project)),
        Command::Ci { action: CiCommand::Run { replay } } => {
            (Action::Ci, replay.selection.clone(), None, Some(replay), None)
        }
        Command::Ci { action: CiCommand::Validate { .. } } => unreachable!("handled above"),
    };
    let mut operation = build_operation(&root, &machine, action, selection, replay, recipe);
    if operation.project.is_none() {
        operation.project = project;
    }

    let state = state_directory(&root);
    let _lock = Lock::acquire(&state)?;
    println!("Host log: {}", state.join("logs").display());
    let report = matrix::execute(&operation)?;
    matrix::record_tool_status(&root, &machine, &report)?;
    matrix::ensure_settled(&report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if !report.succeeded() {
        std::process::exit(1);
    }
    Ok(())
}

fn build_operation(
    root: &std::path::Path,
    machine: &Machine,
    action: Action,
    selection: Selection,
    replay: Option<Replay>,
    recipe: Option<Recipe>,
) -> Operation {
    Operation {
        root: root.to_path_buf(),
        machine: machine.clone(),
        action,
        platforms: platforms(&selection),
        execution: selection.execution,
        project: replay.as_ref().map(|replay| replay.project.clone()),
        framework: recipe.as_ref().and_then(|recipe| recipe.framework),
        command: recipe.as_ref().and_then(|recipe| recipe.command.clone()),
        artifact: recipe.as_ref().and_then(|recipe| recipe.artifact.clone()),
        launch: recipe.as_ref().map(|recipe| recipe.run).unwrap_or(false),
        workflow: replay.as_ref().and_then(|replay| replay.workflow.clone()),
        event: replay.as_ref().map(|replay| replay.event.clone()).unwrap_or_else(|| "workflow_dispatch".to_owned()),
        reference: replay.as_ref().and_then(|replay| replay.reference.clone()),
        result_file: selection.result_file,
        observer: Some(printing_observer()),
    }
}
