//! XCTest process ownership fixture for Rust integration tests only.
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

fn compile(source: &str, output: &Path) -> io::Result<()> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/support")
        .join(source);
    let built = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg("--edition=2024")
        .arg(source)
        .arg("-o")
        .arg(output)
        .output()?;
    if built.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "compile native XCTest fixture: {}",
            String::from_utf8_lossy(&built.stderr)
        )))
    }
}

pub struct XcodebuildHelper {
    executable: PathBuf,
    pid_file: PathBuf,
}

impl XcodebuildHelper {
    pub fn new(artifacts: &Path) -> io::Result<Self> {
        let executable = artifacts.join("xcodebuild-test-helper");
        compile("xcodebuild_helper.rs", &executable)?;
        let pid_file = artifacts.join("xctest-pids");
        fs::write(&pid_file, "")?;
        Ok(Self {
            executable,
            pid_file,
        })
    }

    pub fn command(&self) -> String {
        self.executable.to_string_lossy().into_owned()
    }
}

impl Drop for XcodebuildHelper {
    fn drop(&mut self) {
        if let Ok(pids) = fs::read_to_string(&self.pid_file) {
            for pid in pids
                .lines()
                .filter_map(|pid| pid.parse::<i32>().ok())
                .filter(|pid| *pid > 1)
            {
                #[cfg(unix)]
                // SAFETY: only positive child PIDs recorded by this test fixture
                // are signalled; no name matching or process-group kill occurs.
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
                #[cfg(not(unix))]
                let _ = pid;
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{process::Child, thread, time::Duration};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "native xcodebuild fixture {}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(root.join("bin")).unwrap();
            compile("xcodebuild_fixture.rs", &root.join("bin/xcodebuild")).unwrap();
            Self(root)
        }
        fn command(&self, helper: &XcodebuildHelper) -> Command {
            let mut command = Command::new(helper.command());
            command
                .env("FIXTURE_ROOT", &self.0)
                .env("PATH", self.0.join("bin"));
            command
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn native_helper_preserves_exec_pid_arguments_and_failure_status() {
        let fixture = Fixture::new();
        let helper = XcodebuildHelper::new(&fixture.0).unwrap();
        let output = fixture
            .command(&helper)
            .args([
                "test-without-building",
                "-destination",
                "id=device with spaces",
            ])
            .env("FIXTURE_EXIT", "7")
            .output()
            .unwrap();
        // Disarm cleanup for a child already reaped by output().
        let recorded = fs::read_to_string(&helper.pid_file).unwrap();
        fs::write(&helper.pid_file, "").unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(
            recorded,
            fs::read_to_string(fixture.0.join("actual-pid")).unwrap()
        );
        assert_eq!(
            fs::read_to_string(fixture.0.join("actual-args")).unwrap(),
            "test-without-building\n-destination\nid=device with spaces\n"
        );
    }

    #[test]
    fn builds_and_substring_arguments_do_not_register_test_processes() {
        let fixture = Fixture::new();
        let helper = XcodebuildHelper::new(&fixture.0).unwrap();
        assert!(
            fixture
                .command(&helper)
                .args(["build", "prefix-test-without-building"])
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(fs::read_to_string(&helper.pid_file).unwrap(), "");
    }

    #[test]
    fn failed_ownership_recording_prevents_xcodebuild_start() {
        let fixture = Fixture::new();
        let helper = XcodebuildHelper::new(&fixture.0).unwrap();
        fs::remove_file(&helper.pid_file).unwrap();
        fs::create_dir(&helper.pid_file).unwrap();
        let output = fixture
            .command(&helper)
            .arg("test-without-building")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!fixture.0.join("actual-pid").exists());
    }

    #[test]
    fn cleanup_stops_only_the_native_process_owned_by_this_test() {
        let fixture = Fixture::new();
        let helper = XcodebuildHelper::new(&fixture.0).unwrap();
        let mut unrelated = OwnedChild(Command::new("sleep").arg("60").spawn().unwrap());
        let mut tracked = OwnedChild(
            fixture
                .command(&helper)
                .arg("test-without-building")
                .env("FIXTURE_WAIT", "true")
                .spawn()
                .unwrap(),
        );
        let expected = format!("{}\n", tracked.0.id());
        for _ in 0..100 {
            if fs::read_to_string(fixture.0.join("actual-pid"))
                .ok()
                .as_deref()
                == Some(&expected)
            {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(fs::read_to_string(&helper.pid_file).unwrap(), expected);
        drop(helper);
        for _ in 0..100 {
            if tracked.0.try_wait().unwrap().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(tracked.0.try_wait().unwrap().is_some());
        assert!(unrelated.0.try_wait().unwrap().is_none());
    }
}
