#![recursion_limit = "256"]

extern crate syn;
#[macro_use]
extern crate quote;
extern crate ethabi;
extern crate heck;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

use std::env;
use std::fmt;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::{fs, io};

mod derive;

/// Compiles all solidity files in given directory.
pub fn compile<T: AsRef<Path>>(path: T) -> Result<(), Error> {
    let path = path.as_ref();

    let path = path.canonicalize()
        .map_err(|e| format!("failed to canonicalize: {}: {}", path.display(), e))?;

    let mut c = Command::new("solc");

    c.arg("--combined-json")
        .arg("abi,bin,srcmap,srcmap-runtime,bin-runtime");

    let files = files_by_ext(&path, "sol").expect("failed to list files");

    // nothing to build
    if files.len() == 0 {
        return Ok(());
    }

    for file in files {
        println!("cargo:rerun-if-changed={}", path.join(&file).display());
        c.arg(file);
    }

    let output = c.current_dir(&path)
        .output()
        .map_err(|e| format!("error compiling contracts: {}", e))?;

    if !output.status.success() {
        let stderr = ::std::str::from_utf8(&output.stderr)
            .map_err(|e| format!("failed to decode stderr: {}", e))?;

        return Err(format!("solcjs failed: {:?}\n{}", output.status, stderr).into());
    }

    let output = ::std::str::from_utf8(&output.stdout)
        .map_err(|e| format!("failed to decode stdout: {}", e))?;

    let output: derive::Output =
        serde_json::from_str(&output).map_err(|e| format!("failed to decode output: {}", e))?;

    let result = derive::impl_module(&path, output).map_err(|e| format!("failed to build module: {}", e))?;

    // create contracts.rs
    let out_dir = env::var("OUT_DIR").map_err(|e| format!("OUT_DIR: {}", e))?;

    let path = Path::new(&out_dir).join("contracts.rs");

    let mut fs = fs::File::create(&path)
        .map_err(|e| format!("failed to create file: {}: {}", path.display(), e))?;

    fs.write_all(result.to_string().as_bytes())
        .map_err(|e| format!("failed to write to file: {}: {}", path.display(), e))?;

    return Ok(());
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

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Io(ref e) => write!(fmt, "I/O Error: {}", e),
            Error::Message(ref m) => write!(fmt, "Error: {}", m),
        }
    }
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
