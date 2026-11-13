//! SK-11: what the program mounts, and nothing else. The program runs from
//! memory and never mounts a disk, the stick included.

/// One mount: the source name, the place, the file system type and whether
/// it is mounted read only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mount {
    pub source: &'static str,
    pub target: &'static str,
    pub fstype: &'static str,
    pub read_only: bool,
}

/// The mounts, in order. `securityfs` is read only and is there for one
/// file: the kernel's lockdown mode (SK-06), which the kernel shows only
/// through it.
pub const MOUNTS: [Mount; 5] = [
    Mount {
        source: "proc",
        target: "/proc",
        fstype: "proc",
        read_only: false,
    },
    Mount {
        source: "sysfs",
        target: "/sys",
        fstype: "sysfs",
        read_only: false,
    },
    Mount {
        source: "devtmpfs",
        target: "/dev",
        fstype: "devtmpfs",
        read_only: false,
    },
    Mount {
        source: "efivarfs",
        target: "/sys/firmware/efi/efivars",
        fstype: "efivarfs",
        read_only: true,
    },
    Mount {
        source: "securityfs",
        target: "/sys/kernel/security",
        fstype: "securityfs",
        read_only: true,
    },
];

/// The file systems that may never be mounted (SK-11): every disk file
/// system and the like.
pub const NEVER: &[&str] = &[
    "ext2", "ext3", "ext4", "vfat", "msdos", "exfat", "ntfs", "ntfs3", "btrfs", "xfs", "iso9660",
    "udf", "squashfs", "overlay", "9p", "nfs", "cifs", "fuse",
];
