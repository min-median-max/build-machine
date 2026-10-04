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
