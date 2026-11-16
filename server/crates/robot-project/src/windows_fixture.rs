//! Local fixture ownership, independent of YAML UI actions and MCP drivers.
use std::{io, path::Path};

#[cfg(windows)]
mod native;

pub fn start(app: &Path, state: &Path) -> io::Result<()> {
    #[cfg(windows)]
    return native::start(app, state);
    #[cfg(not(windows))]
    {
        let _ = (app, state);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows fixture requires a Windows host",
        ))
    }
}

pub fn stop(app: &Path, state: &Path) -> io::Result<()> {
    if !state.exists() {
        return Ok(());
    }
    #[cfg(windows)]
    return native::stop(app, state);
    #[cfg(not(windows))]
    {
        let _ = app;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows fixture requires a Windows host",
        ))
    }
}
