//! The native worker.
//!
//! One binary per operating system. The controller places it where the target
//! machine can run it and drives it with a request document; it is also usable
//! directly on that machine, which is what makes a failure inspectable.

mod build;
mod ci;
#[cfg(target_os = "macos")]
mod macos;
mod package;
mod provision;
mod run;
mod stream;
mod workspace;
#[cfg(windows)]
mod win32;

use anyhow::{Context, Result};
use build_machine_core::config::Machine;
use build_machine_core::report::write_atomic;
use build_machine_core::request::WorkRequest;
use clap::{Parser, Subcommand};
use provision::Tools;
use std::path::PathBuf;

/// Markers that let the controller find the structured report in the worker's
/// output, so the report never has to be written to a read-only share.
pub const REPORT_BEGIN: &str = "BUILD_MACHINE_REPORT_BEGIN";
pub const REPORT_END: &str = "BUILD_MACHINE_REPORT_END";

#[derive(Parser)]
#[command(name = "build-machine-worker", about = "Diagnose, provision, build and launch on this machine")]
struct Cli {
    /// The machine definition to work against.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report the actual environment and any missing requirements.
    Doctor,
    /// Install system-wide prerequisites. Requires elevation.
    SetupSystem,
    /// Install every declared prerequisite for this user.
    Setup,
    /// Build a project from a request document.
    Build {
        #[arg(long)]
        request: PathBuf,
        /// Launch the result once it is built.
        #[arg(long)]
        run: bool,
    },
    /// Build and check the installable package.
    Release {
        #[arg(long)]
        request: PathBuf,
    },
    /// Launch the last successful build.
    Run {
        #[arg(long)]
        request: PathBuf,
    },
    /// Replay the supported part of a repository workflow.
    Ci {
        #[arg(long)]
        request: PathBuf,
    },
}

fn machine(cli: &Cli) -> Result<Machine> {
    let path = match &cli.config {
        Some(path) => path.clone(),
        None => std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|parent| parent.join("machine.json")))
            .filter(|path| path.is_file())
            .context("--config로 machine.json 경로를 지정해주세요.")?,
    };
    Machine::load(&path)
}

fn main() {
    if let Err(error) = execute() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}

fn execute() -> Result<()> {
    let cli = Cli::parse();
    let tools = Tools::new(machine(&cli)?)?;
    match &cli.command {
        Command::Doctor => {
            let diagnosis = tools.doctor()?;
            println!("{}", serde_json::to_string_pretty(&diagnosis)?);
            if !diagnosis.ready {
                anyhow::bail!("{}", diagnosis.issues.join("; "));
            }
        }
        Command::SetupSystem => tools.setup_system()?,
        Command::Setup => {
            tools.setup_system()?;
            tools.setup_user()?;
        }
        Command::Build { request, run } => {
            tools.setup_system()?;
            tools.setup_user()?;
            let request = WorkRequest::load(request)?;
            let receipt = build::build(&request, &tools, false)?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
            if *run {
                launch(&receipt, &tools)?;
            }
        }
        Command::Release { request } => {
            tools.setup_system()?;
            tools.setup_user()?;
            let request = WorkRequest::load(request)?;
            let receipt = build::build(&request, &tools, true)?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        }
        Command::Run { request } => {
            let request = WorkRequest::load(request)?;
            let root = build::project_root(&request)?;
            let receipt = run::latest_receipt(&root)?;
            launch(&receipt, &tools)?;
        }
        Command::Ci { request } => {
            let request = WorkRequest::load(request)?;
            let result = ci::replay(&request, &tools)?;
            println!("{REPORT_BEGIN}");
            println!("{}", serde_json::to_string(&result)?);
            println!("{REPORT_END}");
            if !result.success {
                anyhow::bail!("{}", result.error.clone().unwrap_or_else(|| "workflow replay failed".to_owned()));
            }
        }
    }
    Ok(())
}

fn launch(receipt: &build::Receipt, tools: &Tools) -> Result<()> {
    let launched = run::launch(receipt, tools)?;
    let record = provision::workspace()?.join(&receipt.project_key).join("running.json");
    write_atomic(&record, &launched)?;
    println!("{}", serde_json::to_string_pretty(&launched)?);
    Ok(())
}
