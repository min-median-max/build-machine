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
