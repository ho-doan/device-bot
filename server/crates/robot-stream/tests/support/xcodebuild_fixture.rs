//! Native stand-in for xcodebuild used by the harness's own process tests.
use std::{env, fs, path::PathBuf, time::Duration};

fn main() {
    let root = PathBuf::from(env::var_os("FIXTURE_ROOT").unwrap());
    let args: Vec<_> = env::args().skip(1).collect();
    fs::write(root.join("actual-args"), format!("{}\n", args.join("\n"))).unwrap();
    fs::write(root.join("actual-pid"), format!("{}\n", std::process::id())).unwrap();
    if env::var("FIXTURE_WAIT").as_deref() == Ok("true") {
        std::thread::sleep(Duration::from_secs(60));
    }
    std::process::exit(
        env::var("FIXTURE_EXIT")
            .unwrap_or("0".into())
            .parse()
            .unwrap(),
    );
}
