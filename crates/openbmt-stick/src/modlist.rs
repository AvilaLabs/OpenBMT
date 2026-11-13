//! SK-04: the check of `stick/modules.list`. The only modules on the stick are
//! the ones listed, each a file of the kernel package, and none may be a
//! network, wireless, Bluetooth, storage, sound, camera or file system module.
//! The camera modules of `stick/modules-camera.list` (SK-51) are the one
//! exception, held to a list of their own.

/// The folders of the kernel's module tree that may never be listed.
pub const FORBIDDEN: &[&str] = &[
    "kernel/drivers/net/",
    "kernel/net/",
    "kernel/drivers/bluetooth/",
    "kernel/drivers/usb/storage/",
    "kernel/drivers/scsi/",
    "kernel/drivers/ata/",
    "kernel/drivers/nvme/",
    "kernel/fs/",
    "kernel/sound/",
    "kernel/drivers/media/",
];

/// The module paths of the list's text (a path a line, `#` starts a comment),
/// or why the list is refused.
pub fn check_list(text: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        if !line.starts_with("kernel/") || !line.ends_with(".ko.zst") || line.contains("..") {
            return Err(format!("{line} is not a module file of the kernel tree"));
        }
        if let Some(f) = FORBIDDEN.iter().find(|f| line.starts_with(**f)) {
            return Err(format!("{line} is under {f}"));
        }
        out.push(line);
    }
    if out.is_empty() {
        return Err("the list is empty".to_string());
    }
    Ok(out)
}

/// The folders of the kernel's module tree the camera list may name (SK-04).
pub const CAMERA_ALLOWED: &[&str] = &[
    "kernel/drivers/media/usb/uvc/",
    "kernel/drivers/media/common/videobuf2/",
    "kernel/drivers/media/v4l2-core/",
    "kernel/drivers/media/mc/",
    // The helper `uvcvideo` cannot load without; the one file outside the
    // folders above.
    "kernel/drivers/media/common/uvc.ko.zst",
];

/// The module paths of the camera list's text (SK-04, SK-51), or why the list
/// is refused: every module must be under [`CAMERA_ALLOWED`].
pub fn check_camera_list(text: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        if !line.starts_with("kernel/") || !line.ends_with(".ko.zst") || line.contains("..") {
            return Err(format!("{line} is not a module file of the kernel tree"));
        }
        if !CAMERA_ALLOWED.iter().any(|a| line.starts_with(a)) {
            return Err(format!(
                "{line} is not under a folder the camera list may name"
            ));
        }
        out.push(line);
    }
    if out.is_empty() {
        return Err("the list is empty".to_string());
    }
    Ok(out)
}
