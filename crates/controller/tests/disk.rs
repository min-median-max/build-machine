//! Reclaiming the space a virtual machine's guest frees.

use build_machine_controller::disk::disks_without_online_compact;

/// The guest's TRIM gives blocks back to the Mac only when Parallels compacts
/// the disk online; a disk without it keeps every block it ever held.
#[test]
fn a_disk_without_online_compaction_is_found_in_the_vm_description() {
    let info = "\
  memory size=12288Mb auto=off
  hdd0 (+) sata:0 image='/Users/x/Parallels/Ubuntu.pvm/harddisk1.hdd' type='expanded' 65536Mb online-compact=on
  hdd1 (+) sata:2 image='/Users/x/Parallels/Ubuntu.pvm/harddisk2.hdd' type='expanded' 1024Mb online-compact=off
  hdd2 (+) nvme:0 image='/Users/x/Parallels/Ubuntu.pvm/harddisk3.hdd' type='plain' 1024Mb
  cdrom0 (+) sata:1 image='' state=disconnected
";
    assert_eq!(disks_without_online_compact(info), ["hdd1", "hdd2"]);
    assert!(disks_without_online_compact("  hdd0 (+) sata:0 image='a' online-compact=on\n").is_empty());
}

/// The images whose allocation the compaction is measured by.
#[test]
fn the_disk_images_are_read_from_the_vm_description() {
    use build_machine_controller::disk::disk_images;
    let info = "  hdd0 (+) sata:0 image='/Users/x/Parallels/Ubuntu 26.04 ARM64.pvm/harddisk1.hdd' type='expanded' 65536Mb online-compact=on\n  cdrom0 (+) sata:1 image='' state=disconnected\n";
    assert_eq!(disk_images(info), [std::path::PathBuf::from("/Users/x/Parallels/Ubuntu 26.04 ARM64.pvm/harddisk1.hdd")]);
}

/// An image's size on the Mac is the blocks its `.hds` files allocate; their
/// apparent size stays at its largest.
#[test]
fn an_image_is_measured_by_its_allocated_blocks() {
    use build_machine_controller::disk::allocation;
    let directory = tempfile::tempdir().unwrap();
    let image = directory.path().join("harddisk1.hdd");
    std::fs::create_dir_all(&image).unwrap();
    std::fs::write(image.join("DiskDescriptor.xml"), vec![1u8; 4096]).unwrap();
    let data = image.join("harddisk1.hdd.0.{guid}.hds");
    std::fs::write(&data, vec![7u8; 1 << 20]).unwrap();
    std::fs::OpenOptions::new().write(true).open(&data).unwrap().set_len(64 << 20).unwrap();
    let measured = allocation(&image).unwrap();
    assert_eq!(measured.apparent_bytes, 64 << 20);
    assert!(measured.allocated_bytes >= 1 << 20 && measured.allocated_bytes < 8 << 20, "{measured:?}");
}

/// The VM is stopped for offline compaction only when it can be started
/// again. Parallels refuses to start a VM without free host space for its
/// memory ("There's not enough disk space available to start … Free at least
/// 8175 MB", replay 4 of orm, a 16384 MB VM with 7 GB free), which left the
/// machine stopped.
#[test]
fn the_vm_is_stopped_only_with_room_to_start_it_again() {
    use build_machine_controller::disk::{available_bytes, memory_mb, start_room};
    let info = "  cpu cpus=4 auto=off\n  memory size=16384Mb auto=off\n  hdd0 (+) sata:0 image='/x.hdd'\n";
    assert_eq!(memory_mb(info), Some(16384));
    let df = "Filesystem 1024-blocks      Used Available Capacity iused ifree %iused  Mounted on\n/dev/disk3s1 482797652 431956820 7340032 99% 3190789 79328680 4% /System/Volumes/Data\n";
    assert_eq!(available_bytes(df), Some(7340032 * 1024));
    let need = (16384 + 1024) * 1024 * 1024;
    let refused = start_room(7 * 1024 * 1024 * 1024, 16384).unwrap_err();
    assert!(refused.contains(&need.to_string()) && refused.contains("16384"), "{refused}");
    assert_eq!(start_room(need, 16384), Ok(need));
}
