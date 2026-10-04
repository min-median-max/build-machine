//! Giving the space a guest frees back to the Mac.
//!
//! A Parallels disk grows as the guest writes and keeps the blocks it holds.
//! It shrinks only through two steps together: the guest discards the blocks
//! its file system no longer uses (`fstrim`, which the worker's `reclaim`
//! runs), and Parallels compacts the image online, punching those blocks out
//! of the `.hds` file. The file's apparent size stays at its high-water mark;
//! the blocks it allocates on the Mac are what shrink.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// The hard disks of a `prlctl list -i` description that do not compact online.
pub fn disks_without_online_compact(info: &str) -> Vec<String> {
    info.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("hdd"))
        .filter(|line| !line.split_whitespace().any(|word| word == "online-compact=on"))
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect()
}

/// Turn on online compaction for every disk of the VM that lacks it.
pub fn ensure_online_compact(prlctl: &Path, vm: &str) -> Result<Vec<String>> {
    let output = Command::new(prlctl).args(["list", "-i", vm]).output().context("Parallels prlctl을 실행하지 못했어요.")?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let disks = disks_without_online_compact(&String::from_utf8_lossy(&output.stdout));
    for disk in &disks {
        let status = Command::new(prlctl)
            .args(["set", vm, "--device-set", disk, "--online-compact", "on"])
            .status()
            .context("Parallels prlctl을 실행하지 못했어요.")?;
        if !status.success() {
            anyhow::bail!("{vm}의 {disk}에 online compaction을 켜지 못했어요.");
        }
    }
    Ok(disks)
}

/// The disk images of a `prlctl list -i` description.
pub fn disk_images(info: &str) -> Vec<std::path::PathBuf> {
    info.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("hdd"))
        .filter_map(|line| {
            let start = line.find("image='")? + "image='".len();
            let end = line[start..].find('\'')? + start;
            Some(std::path::PathBuf::from(&line[start..end]))
        })
        .filter(|path| !path.as_os_str().is_empty())
        .collect()
}

/// What an image takes on the Mac.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub allocated_bytes: u64,
    pub apparent_bytes: u64,
}

/// Measure an image by the blocks its `.hds` data files allocate and by
/// their length.
pub fn allocation(image: &Path) -> Result<Allocation> {
    use std::os::unix::fs::MetadataExt;
    let mut measured = Allocation { allocated_bytes: 0, apparent_bytes: 0 };
    let mut found = false;
    for entry in std::fs::read_dir(image).with_context(|| format!("디스크 이미지를 읽지 못했어요: {}", image.display()))? {
        let entry = entry?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("hds") {
            continue;
        }
        let metadata = entry.metadata()?;
        measured.allocated_bytes += metadata.blocks() * 512;
        measured.apparent_bytes += metadata.len();
        found = true;
    }
    if !found {
        anyhow::bail!("디스크 이미지에 .hds 파일이 없어요: {}", image.display());
    }
    Ok(measured)
}

/// The `prl_disk_tool` beside `prlctl`.
pub fn disk_tool(prlctl: &Path) -> std::path::PathBuf {
    prlctl.with_file_name("prl_disk_tool")
}

/// Compact a VM's disks with the VM stopped, after its guest discarded its
/// free blocks, and start it again.
///
/// Parallels' online compaction after the guest's TRIM gave space back once in
/// three measured runs on 2026-10-04; compaction of the stopped VM gave back
/// every trimmed block (35,398,656 KiB to 27,033,600 KiB in 7 seconds). The
/// VM is stopped, so the caller holds the machine lock and no other work of
/// this machine is running.
pub fn compact_offline(
    prlctl: &Path,
    vm: &str,
    log: &crate::oplog::OperationLog,
) -> Result<Vec<build_machine_core::report::DiskCompaction>> {
    let run = |program: &Path, arguments: &[&str]| -> Result<()> {
        log.command(&format!("{} {}", program.display(), arguments.join(" ")));
        let output = Command::new(program).args(arguments).output().with_context(|| format!("{}을 실행하지 못했어요.", program.display()))?;
        let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        log.note(text.trim());
        if !output.status.success() {
            anyhow::bail!("{} {} failed: {}", program.display(), arguments.join(" "), text.trim());
        }
        Ok(())
    };
    let info = Command::new(prlctl).args(["list", "-i", vm]).output().context("Parallels prlctl을 실행하지 못했어요.")?;
    let images = disk_images(&String::from_utf8_lossy(&info.stdout));
    let before: Vec<Allocation> = images.iter().map(|image| allocation(image)).collect::<Result<_>>()?;
    run(prlctl, &["stop", vm])?;
    let compacted: Result<()> = images.iter().try_for_each(|image| run(&disk_tool(prlctl), &["compact", "--hdd", &image.to_string_lossy()]));
    // The VM is started again whatever the compaction did.
    run(prlctl, &["start", vm])?;
    compacted?;
    let mut results = Vec::new();
    for (image, before) in images.iter().zip(before) {
        let after = allocation(image)?;
        log.note(&format!(
            "DISK {} allocated {} -> {} bytes, apparent {} -> {} bytes",
            image.display(),
            before.allocated_bytes,
            after.allocated_bytes,
            before.apparent_bytes,
            after.apparent_bytes
        ));
        results.push(build_machine_core::report::DiskCompaction {
            image: image.to_string_lossy().into_owned(),
            allocated_before: before.allocated_bytes,
            allocated_after: after.allocated_bytes,
            apparent_before: before.apparent_bytes,
            apparent_after: after.apparent_bytes,
        });
    }
    Ok(results)
}

/// How long a started VM may take until its guest runs a command.
pub const GUEST_READY: std::time::Duration = std::time::Duration::from_secs(300);

/// Wait until the guest runs a command after the VM started. Parallels
/// reports the VM running before its Tools accept a command; each attempt and
/// its error are logged, and the wait ends at [`GUEST_READY`].
pub fn wait_for_guest(prlctl: &Path, vm: &str, log: &crate::oplog::OperationLog) -> Result<()> {
    let started = std::time::Instant::now();
    let mut attempt = 0;
    loop {
        attempt += 1;
        let output = Command::new(prlctl).args(["exec", vm, "true"]).output().context("Parallels prlctl을 실행하지 못했어요.")?;
        if output.status.success() {
            log.note(&format!("GUEST {vm} runs commands after {} s ({attempt} attempts)", started.elapsed().as_secs()));
            return Ok(());
        }
        let message = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        log.note(&format!("GUEST {vm} not ready (attempt {attempt}): {}", message.trim()));
        if started.elapsed() >= GUEST_READY {
            anyhow::bail!("{vm} did not run a command within {} s after it started: {}", GUEST_READY.as_secs(), message.trim());
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}
