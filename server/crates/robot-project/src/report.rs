use std::{
    fs,
    io::{self, Write},
    path::Path,
};

/// Preserve real case reports; warmup/preflight reports do not satisfy a case.
pub fn ensure_report(directory: &Path) -> io::Result<bool> {
    if directory.join("setup/junit.xml").is_file() {
        return Ok(false);
    }
    if directory.is_dir() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && entry.file_name().to_string_lossy().starts_with("case-")
                && entry.path().join("junit.xml").is_file()
            {
                return Ok(false);
            }
        }
    }
    let setup = directory.join("setup");
    fs::create_dir_all(&setup)?;
    let mut file = match fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(setup.join("junit.xml"))
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => return Err(error),
    };
    file.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuite name=\"project-ci\" tests=\"1\" errors=\"1\"><testcase name=\"setup-or-evidence\"><error message=\"Project setup or evidence validation failed; inspect job and runner logs.\"/></testcase></testsuite>\n")?;
    Ok(true)
}
