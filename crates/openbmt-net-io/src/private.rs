//! Files and directories that only the account that runs the program can
//! use: mode 0600 for a file and 0700 for a directory on unix, whatever the
//! umask says. Every file the Box, the relay and the counter write goes
//! through here, so that none is left readable by others.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Open `path` for writing, empty, and with mode 0600. A file that is there
/// is emptied and its mode set to 0600 as well, so that a leftover of an
/// earlier run with a wider mode does not keep it.
pub fn create_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

/// Open `path` for reading and writing, made with mode 0600 if it is not
/// there, and not emptied if it is.
pub fn open_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Make `path` and the parents it lacks, each with mode 0700. A directory
/// that is there is left as it is.
pub fn create_dir_all(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    fn mode_of(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let base = std::env::var_os("CARGO_TARGET_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("private-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_new_file_is_0600_and_a_new_directory_0700_whatever_the_umask() {
        let dir = scratch("new");
        let nested = dir.join("a/b");
        create_dir_all(&nested).unwrap();
        assert_eq!(mode_of(&dir.join("a")), 0o700);
        assert_eq!(mode_of(&nested), 0o700);
        let path = nested.join("f");
        create_file(&path).unwrap().write_all(b"x").unwrap();
        assert_eq!(mode_of(&path), 0o600);
        let lock = nested.join("l");
        drop(open_file(&lock).unwrap());
        assert_eq!(mode_of(&lock), 0o600);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_leftover_with_a_wider_mode_is_made_0600_and_emptied() {
        let dir = scratch("stale");
        let path = dir.join("f.tmp");
        std::fs::write(&path, b"old bytes").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        drop(create_file(&path).unwrap());
        assert_eq!(mode_of(&path), 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
