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
