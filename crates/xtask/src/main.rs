//! Development tasks for this repository.
//!
//! Building the desktop application needs the worker binaries staged first,
//! because the bundle declares them and fails without them. That ordering is
//! the reason this exists rather than a bare `cargo tauri build`.

mod guest;
mod workers;

use anyhow::{bail, Context, Result};
use build_machine_core::Platform;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Parser)]
#[command(name = "xtask", about = "Development tasks for the build machine")]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Run the frontend and the Rust application together.
    Dev,
    /// Build the macOS application bundle.
    Build {
        /// Open the built application.
        #[arg(long)]
        run: bool,
    },
    /// Run every test: Rust, the frontend build and the browser tests.
    Test,
    /// Build the worker binaries.
    ///
    /// The host worker is built here. A guest worker is built inside its own
    /// virtual machine, which is how an end-to-end check runs without waiting
    /// for a tagged release.
    Worker {
        #[arg(long = "os", num_args = 1.., value_parser = parse_platform)]
        platforms: Vec<Platform>,
    },
}

fn parse_platform(value: &str) -> Result<Platform, String> {
    value.parse().map_err(|error: anyhow::Error| error.to_string())
}

pub fn root() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.ancestors().nth(2).map(Path::to_path_buf).context("워크스페이스 루트를 찾지 못했어요.")
}

pub fn run(program: &str, arguments: &[&str], working: &Path) -> Result<()> {
    println!("> {program} {}", arguments.join(" "));
    let status = Command::new(program)
        .args(arguments)
        .current_dir(working)
        .status()
        .with_context(|| format!("{program}을 실행하지 못했어요."))?;
    if !status.success() {
        bail!("{program} failed with exit code {}.", status.code().unwrap_or(-1));
    }
    Ok(())
}

fn main() {
    if let Err(error) = execute() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}

fn execute() -> Result<()> {
    let cli = Cli::parse();
    let root = root()?;
    let gui = root.join("gui");
    match cli.command {
        Task::Worker { platforms } => {
            let platforms = if platforms.is_empty() { vec![Platform::host()?] } else { platforms };
            workers::build(&root, &platforms)
        }
        Task::Dev => {
            run("pnpm", &["install", "--frozen-lockfile"], &gui)?;
            workers::build(&root, &[Platform::host()?])?;
            run("pnpm", &["exec", "tauri", "dev"], &gui)
        }
        Task::Test => {
            run("cargo", &["test", "--workspace", "--locked"], &root)?;
            run("cargo", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"], &root)?;
            run("pnpm", &["install", "--frozen-lockfile"], &gui)?;
            run("pnpm", &["run", "build"], &gui)?;
            run("pnpm", &["exec", "playwright", "install", "chromium"], &gui)?;
            run("pnpm", &["test"], &gui)
        }
        Task::Build { run: open } => {
            if !cfg!(target_os = "macos") {
                bail!("The desktop application is built on macOS.");
            }
            run("pnpm", &["install", "--frozen-lockfile"], &gui)?;
            workers::build(&root, &Platform::ALL)?;
            run("pnpm", &["exec", "tauri", "build", "--ci", "--no-sign", "--bundles", "app", "--", "--locked"], &gui)?;
            let application = root.join("target/release/bundle/macos/Build Machine.app");
            if !application.is_dir() {
                bail!("The desktop application bundle was not produced.");
            }
            println!("Application: {}", application.display());
            if open {
                run("open", &[&application.to_string_lossy()], &root)?;
            }
            Ok(())
        }
    }
}
