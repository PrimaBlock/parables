#![recursion_limit = "256"]

extern crate proc_macro;
extern crate syn;
#[macro_use]
extern crate quote;
extern crate ethabi;
extern crate heck;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

mod derive;

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::Command;

#[proc_macro_derive(ParablesContracts, attributes(parables, parables_contract))]
pub fn ethabi_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast: syn::DeriveInput = syn::parse(input).expect("valid token stream");
    let options = get_options(&ast.attrs).expect("bad attribute `parables`");
    let gen = compile(options).expect("input to compile");
    gen.into()
}

fn get_options(attrs: &[syn::Attribute]) -> Result<derive::Options, Error> {
    let mut path = None;
    let mut contracts = Vec::new();

    for attr in attrs {
        let meta = match attr.interpret_meta() {
            Some(meta) => meta,
            None => {
                continue;
            }
        };

        if meta.name() == "parables" {
            path = Some(decode_parables(meta)?);
            continue;
        }

        if meta.name() == "parables_contract" {
            contracts.extend(decode_parables_contract(meta)?);
            continue;
        }
    }

    let path = path.ok_or_else(|| "Missing attribute parables(path = ...)")?;

    return Ok(derive::Options { path, contracts });

    fn decode_parables(meta: syn::Meta) -> Result<PathBuf, Error> {
        let mut path = None;

        let values = match meta {
            syn::Meta::List(list) => list.nested,
            _ => return Err("Unexpected meta item in parables(...)".into()),
        };

        for v in values {
            let v = match v {
                syn::NestedMeta::Meta(meta) => meta,
                _ => return Err("Expected nested meta in parables(...)".into()),
            };

            if v.name() == "path" {
                if let syn::Meta::NameValue(ref name_value) = v {
                    if let syn::Lit::Str(ref value) = name_value.lit {
                        path = Some(PathBuf::from(value.value()));
                        continue;
                    }
                }
            }

            return Err(format!("Bad attribute `{}` in parables(...)", v.name()).into());
        }

        let path = path.ok_or_else(|| "Missing attribute parables(path = ...)")?;
        Ok(path)
    }

    fn decode_parables_contract(meta: syn::Meta) -> Result<Vec<derive::ParablesContract>, Error> {
        let values = match meta {
            syn::Meta::List(list) => list.nested,
            _ => return Err("Unexpected meta item in parables_contract(...)".into()),
        };

        let mut contracts = Vec::new();

        for v in values {
            let meta = match v {
                syn::NestedMeta::Meta(meta) => meta,
                _ => return Err("Bad argument to parables_contract(...)".into()),
            };

            let name_value = match meta {
                syn::Meta::NameValue(ref name_value) => name_value,
                _ => return Err("Bad argument to parables_contract(...)".into()),
            };

            let item = meta.name().to_string();

            let argument = match name_value.lit {
                syn::Lit::Str(ref value) => value.value(),
                _ => return Err("Bad argument to parables_contract(...)".into()),
            };

            let mut parts = argument.split(":");

            let file = match parts.next() {
                Some(file) => file.to_string(),
                _ => {
                    return Err(format!("Bad argument to parables_contract({} = ...)", item).into())
                }
            };

            let entry = match parts.next() {
                Some(entry) => entry.to_string(),
                _ => {
                    return Err(format!("Bad argument to parables_contract({} = ...)", item).into())
                }
            };

            contracts.push(derive::ParablesContract { item, file, entry });
        }

        Ok(contracts)
    }
}

/// Compiles all solidity files in given directory.
fn compile(options: derive::Options) -> Result<quote::Tokens, Error> {
    let root = match ::std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(e) => return Err(e.to_string().into()),
    };

    let path = root.join(&options.path);

    let mut c = Command::new("solc");

    c.arg("--combined-json")
        .arg("abi,bin,srcmap,srcmap-runtime,bin-runtime,ast");

    for contract in &options.contracts {
        let path = path.join(&contract.file);

        if !path.is_file() {
            panic!("No such file: {}", path.display());
        }

        c.arg(&contract.file);
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

    let result = derive::impl_module(&path, output, options.contracts)
        .map_err(|e| format!("failed to build module: {}", e))?;

    Ok(result)
}

#[derive(Debug)]
enum Error {
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
