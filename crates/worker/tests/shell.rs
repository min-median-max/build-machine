//! Handing a workflow `run:` block to a shell.
//!
//! A `run:` block is usually several lines, and every line has to run. A
//! Windows replay of AIRDATA reported its test stage as passed in 3.4 seconds
//! with no output at all, while the same stage on Linux ran for two minutes
//! and printed 38 test results. The block was
//!
//! ```text
//! pnpm exec tsc --noEmit
//! cargo test --locked --manifest-path src-tauri/Cargo.toml
//! ```
//!
//! and `cmd.exe /d /s /c` stops at the first newline: only `tsc` ran, which
//! prints nothing when it succeeds, and `cargo test` was dropped without a
//! trace. The gate passed having tested nothing.

use build_machine_worker::build::{encoded_powershell, powershell_script, shell_invocation};

const BLOCK: &str = "pnpm exec tsc --noEmit\ncargo test --locked --manifest-path src-tauri/Cargo.toml";

/// Undo the encoding, to read what the shell would actually be given.
fn decode(encoded: &str) -> String {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
    let words: Vec<u16> = bytes.as_chunks::<2>().0.iter().copied().map(u16::from_le_bytes).collect();
    String::from_utf16(&words).unwrap()
}

#[test]
fn every_line_of_a_block_reaches_the_windows_shell() {
    let script = decode(&encoded_powershell(BLOCK));
    assert!(script.contains("pnpm exec tsc --noEmit"), "the first line is missing:\n{script}");
    assert!(
        script.contains("cargo test --locked --manifest-path src-tauri/Cargo.toml"),
        "the line after the first was dropped, which is the defect:\n{script}"
    );
}

#[test]
fn a_failing_command_carries_its_exit_code_out() {
    // PowerShell exits 0 even when the last native command failed, so a step
    // would pass on a failing `cargo test` unless the code is carried out.
    let script = powershell_script("cargo test");
    assert!(script.contains("exit $LASTEXITCODE"), "a failure would be reported as success:\n{script}");
}

#[test]
fn the_report_carries_output_rather_than_clixml() {
    // A redirected PowerShell stream carries progress records as CLIXML no
    // matter the output format, and they landed in the stored step output.
    let script = powershell_script("cargo test");
    assert!(script.contains("$ProgressPreference = 'SilentlyContinue'"), "progress noise reaches the report");
}

#[test]
fn the_block_is_encoded_rather_than_quoted_onto_a_command_line() {
    // Quoting is what loses lines and trips over embedded quotes. Encoding the
    // script carries it verbatim, and leaves no script file on the machine.
    let (program, arguments) = shell_invocation(BLOCK);
    if cfg!(windows) {
        assert!(program.contains("powershell"), "unexpected shell: {program}");
        assert!(arguments.iter().any(|argument| argument == "-EncodedCommand"));
        assert!(
            !arguments.iter().any(|argument| argument.contains('\n')),
            "a raw newline on the command line is what cmd.exe truncated at"
        );
        // `pnpm` resolves to `pnpm.ps1`, which the default execution policy
        // refuses to load: the first real replay failed on exactly this.
        assert!(arguments.windows(2).any(|pair| pair == ["-ExecutionPolicy", "Bypass"]));
        // Otherwise the step's output reaches the report as CLIXML.
        assert!(arguments.windows(2).any(|pair| pair == ["-OutputFormat", "Text"]));
    } else {
        assert_eq!(program, "/bin/sh");
        assert_eq!(arguments, vec!["-eu".to_owned(), "-c".to_owned(), BLOCK.to_owned()]);
    }
}
