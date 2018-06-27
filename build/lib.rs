use std::path::Path;
use std::process::Command;
use std::{fs, io};

/// Compiles all solidity files in given directory.
pub fn compile<T: AsRef<Path>>(path: T) -> Result<(), Error> {
    let path = path.as_ref();

    let mut c = Command::new("solcjs");
    c.arg("--bin").arg("--abi");

    let files = files_by_ext(path, "sol").expect("failed to list files");

    // nothing to build
    if files.len() == 0 {
        return Ok(());
    }

    for file in files {
        println!("cargo:rerun-if-changed={}", path.join(&file).display());
        c.arg(file);
    }

    let status = c.current_dir(path)
        .status()
        .map_err(|e| format!("error compiling contracts: {}", e))?;

    if !status.success() {
        return Err(format!("solcjs failed: {:?}", status).into());
    }

    Ok(())
}

fn files_by_ext(path: &Path, expected: &str) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();

    if !path.is_dir() {
        return Ok(out);
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let p = entry.path();

        let ext = match p.extension() {
            None => continue,
            Some(ext) => ext.to_str().ok_or_else(|| "not a valid extension")?,
        };

        if ext == expected {
            let file_name = match p.file_name() {
                None => continue,
                Some(file_name) => file_name.to_str().ok_or_else(|| "not a valid file name")?,
            };

            out.push(file_name.to_string());
        }
    }

    Ok(out)
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Message(String),
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::Io(value)
    }
}

impl From<&'static str> for Error {
    fn from(value: &'static str) -> Self {
        Error::Message(value.to_string())
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Error::Message(value)
    }
}
