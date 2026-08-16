use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use crate::platform::SingleInstanceGuard;

struct Guard {
    _file: File,
}

impl SingleInstanceGuard for Guard {}

pub fn acquire() -> anyhow::Result<Option<Box<dyn SingleInstanceGuard>>> {
    let path = lock_path();
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| anyhow::anyhow!("open instance lock {}: {error}", path.display()))?;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(Some(Box::new(Guard { _file: file })));
    }
    let error = std::io::Error::last_os_error();
    if matches!(error.raw_os_error(), Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN)
    {
        return Ok(None);
    }
    Err(error).map_err(|error| anyhow::anyhow!("lock instance file {}: {error}", path.display()))
}

fn lock_path() -> PathBuf {
    let directory = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    directory.join(format!("patrick-star2-{}.lock", unsafe { libc::geteuid() }))
}
