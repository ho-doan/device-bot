//! Standalone native fixture compiled only by the live-test harness.
use std::{env, ffi::OsStr, fs::OpenOptions, io, io::Write, process::Command};

#[cfg(unix)]
fn run() -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args
        .iter()
        .any(|arg| arg == OsStr::new("test-without-building"))
    {
        let executable = env::current_exe()?;
        let pid_file = executable.parent().unwrap().join("xctest-pids");
        let mut pids = OpenOptions::new()
            .create(true)
            .append(true)
            .open(pid_file)?;
        writeln!(pids, "{}", std::process::id())?;
        pids.sync_data()?;
    }
    // Replace this fixture with the actual xcodebuild process, retaining its PID
    // and OS arguments. A failed ownership write never reaches this operation.
    Err(Command::new("xcodebuild").args(args).exec())
}

fn main() {
    #[cfg(unix)]
    if let Err(error) = run() {
        eprintln!("xcodebuild test helper: {error}");
        std::process::exit(127);
    }
    #[cfg(not(unix))]
    panic!("the XCTest test helper requires a Unix host");
}
