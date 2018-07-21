use error::Error;
use ethereum_types::Address;
use parity_evm;
use source_map::SourceMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// hex lookup table
///
/// each index maps the ascii value of a byte to its corresponding hexadecimal value.
static HEX: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 0, 0, 0, 0, 0,
    0, 10, 11, 12, 13, 14, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 10, 11, 12, 13, 14, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0,
];

/// A solidity bytecode linker.
#[derive(Debug, Clone)]
pub struct Linker {
    /// Known linkable objects by item.
    objects_by_item: HashMap<String, Address>,
    /// Addresses to known items.
    item_by_address: HashMap<Address, String>,
    /// Known linkable objects by path.
    objects_by_path: HashMap<String, Address>,
    /// Known source maps by item.
    sources: HashMap<String, Arc<(SourceMap, HashMap<usize, usize>)>>,
    /// Known runtime source maps by item.
    runtime_sources: HashMap<String, Arc<(SourceMap, HashMap<usize, usize>)>>,
    /// Known sources.
    source_list: Option<Arc<Vec<PathBuf>>>,
}

impl Linker {
    /// Construct a new linker.
    pub fn new() -> Self {
        Self {
            objects_by_item: HashMap::new(),
            item_by_address: HashMap::new(),
            objects_by_path: HashMap::new(),
            sources: HashMap::new(),
            runtime_sources: HashMap::new(),
            source_list: None,
        }
    }

    pub fn register_source_list(&mut self, source_list: Vec<PathBuf>) {
        self.source_list = Some(Arc::new(source_list));
    }

    /// Register a runtime source.
    pub fn register_source(
        &mut self,
        item: String,
        source_map: SourceMap,
        offsets: HashMap<usize, usize>,
    ) {
        self.sources.insert(item, Arc::new((source_map, offsets)));
    }

    /// Find a corresponding source map for the given address.
    pub fn find_source(
        &self,
        address: Address,
    ) -> Option<(Arc<(SourceMap, HashMap<usize, usize>)>)> {
        self.item_by_address
            .get(&address)
            .and_then(|item| self.sources.get(item))
            .map(Arc::clone)
    }

    /// Register a runtime source.
    pub fn register_runtime_source(
        &mut self,
        item: String,
        source_map: SourceMap,
        offsets: HashMap<usize, usize>,
    ) {
        self.runtime_sources
            .insert(item, Arc::new((source_map, offsets)));
    }

    /// Find a corresponding runtime source map for the given address.
    pub fn find_runtime_source(
        &self,
        address: Address,
    ) -> Option<(Arc<(SourceMap, HashMap<usize, usize>)>)> {
        self.item_by_address
            .get(&address)
            .and_then(|item| self.runtime_sources.get(item))
            .map(Arc::clone)
    }

    /// Find the corresponding file to an index.
    pub fn find_file(&self, index: u32) -> Option<&Path> {
        self.source_list
            .as_ref()
            .and_then(|source_list| source_list.get(index as usize).map(|p| p.as_ref()))
    }

    /// Register an address for an item.
    pub fn register_item(&mut self, item: String, address: Address) {
        self.objects_by_item.insert(item.clone(), address);
        self.item_by_address.insert(address, item);
    }

    /// Register an address for a path.
    pub fn register_path(&mut self, path: String, address: Address) {
        self.objects_by_path.insert(path, address);
    }

    /// Decoded the given code into instruction offsets.
    pub fn decode_offsets(&self, mut code: &str) -> Result<HashMap<usize, usize>, Error> {
        code = code.trim();

        let mut out = HashMap::new();

        let mut n = 0;
        let mut offset = 0;

        out.insert(n, offset);

        while code.len() >= 2 {
            // swarm hash, ignore
            if code.len() == 43 * 2 {
                code = &code[43 * 2..];
                continue;
            }

            match code[..2].as_bytes() {
                &[a, b] if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() => {
                    let mut o = 0u8;
                    o += HEX[a as usize] << 4;
                    o += HEX[b as usize];
                    code = &code[2..];

                    let info =
                        parity_evm::Instruction::from_u8(o).ok_or_else(|| Error::BadInputPos {
                            position: n * 2,
                            message: "bad instruction",
                        })?;

                    n += 1;
                    offset += 1;

                    if let Some(bytes) = info.push_bytes() {
                        if code.len() < 2 * bytes {
                            return Err(Error::BadInputPos {
                                position: n * 2,
                                message: "expected push number of bytes",
                            });
                        }

                        n += bytes;
                        code = &code[2 * bytes..];
                    }

                    out.insert(n, offset);
                }
                _ => {
                    return Err(Error::BadInputPos {
                        position: n * 2,
                        message: "two hex digits",
                    });
                }
            }
        }

        Ok(out)
    }

    /// Decode and link the given bytecode.
    ///
    /// The bytecode is represented in ascii, where each byte corresponds to a hex character.
    ///
    /// Entries to be linked are designated with two underscores `__`, these should be replaced
    /// with an address corresponding to the linked object.
    ///
    /// All other entries should be left preserved.
    pub fn link(&self, mut code: &str) -> Result<Vec<u8>, Error> {
        code = code.trim();

        let mut output = Vec::new();
        let mut n = 0usize;

        // read input in pairs.
        while code.len() >= 2 {
            match code[..2].as_bytes() {
                // section to link
                &[b'_', b'_'] => {
                    if code.len() < 40 {
                        return Err(Error::BadInputPos {
                            position: n,
                            message: "expected link section at position {}, but remaining code is too small",
                        });
                    }

                    let (path, item) = decode_linked(&code[..40])?;

                    let address = match item {
                        Some(item) => self.objects_by_item.get(item).ok_or_else(|| {
                            Error::NoLinkerItem {
                                item: item.to_string(),
                            }
                        })?,
                        None => self.objects_by_path.get(path).ok_or_else(|| {
                            Error::NoLinkerPath {
                                path: path.to_string(),
                            }
                        })?,
                    };

                    output.extend(address.iter());

                    code = &code[40..];
                    n += 40;
                }
                &[a, b] if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() => {
                    let mut o = 0u8;
                    o += HEX[a as usize] << 4;
                    o += HEX[b as usize];
                    output.push(o);
                    code = &code[2..];
                    n += 2;
                }
                _ => {
                    return Err(Error::BadInputPos {
                        position: n,
                        message: "expected `__` or two hex digits",
                    });
                }
            }
        }

        return Ok(output);

        /// Decode a single 40-byte linking section.
        ///
        /// Generally has the structure `<path>:<item>`, where `<item>` is optional since it might
        /// not fit within the section.
        fn decode_linked(chunk: &str) -> Result<(&str, Option<&str>), Error> {
            let mut chunk = chunk.trim_matches('_');

            let sep = match chunk.find(':') {
                None => return Ok((chunk, None)),
                Some(sep) => sep,
            };

            let path = &chunk[..sep];
            chunk = &chunk[sep..];

            let mut it = chunk.char_indices();
            it.next();

            let n = match it.next() {
                None => return Ok((path, None)),
                Some((n, _)) => n,
            };

            Ok((path, Some(&chunk[n..])))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Linker;

    extern crate hex;

    #[test]
    fn test_linker() {
        let linker = Linker::new();

        let a = hex::decode(b"01234567789abcdefABCDEFF").expect("bad hex decode");
        let b = linker
            .link("01234567789abcdefABCDEFF")
            .expect("bad link decode");

        linker.link("FF").expect("bad link decode");
        assert_eq!(a, b);
    }

    #[test]
    fn test_linker_against_contract_a() {
        let mut linker = Linker::new();
        linker.register_item("SimpleLib".to_string(), 0x342a.into());

        let out = linker
            .link(include_str!("tests/a.bin"))
            .expect("bad link decode");

        // already linked should have no effect.
        let linked = linker
            .link(include_str!("tests/linked_a.bin"))
            .expect("bad link decode");

        assert_eq!(linked, out);
    }

    #[test]
    fn test_decode_instruction_offsets() {
        let linker = Linker::new();

        let decoded = linker
            .decode_offsets(include_str!("tests/runtime.bin"))
            .expect("bad decode");

        println!("decoded: {:?}", decoded);
    }
}
