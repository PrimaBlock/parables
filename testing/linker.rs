use error::Error;
use ethereum_types::Address;
use parity_evm;
use source_map::SourceMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// All necessary source information to perform tracing.
#[derive(Debug)]
pub struct Source {
    /// The source map for the given source.
    pub source_map: SourceMap,
    /// The decoded offsets for the given source, from program counter to instruction offset.
    pub offsets: HashMap<usize, usize>,
}

/// hex lookup table
///
/// each index maps the ascii value of a byte to its corresponding hexadecimal value.
static HEX: [i8; 256] = [
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, -1, -1, -1, -1, -1, -1, -1, 10, 11, 12, 13, 14, 15, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 10,
    11, 12, 13, 14, 15, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
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
    sources: HashMap<String, Arc<Source>>,
    /// Known runtime source maps by item.
    runtime_sources: HashMap<String, Arc<Source>>,
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
    pub fn register_source(&mut self, item: String, source: Source) {
        self.sources.insert(item, Arc::new(source));
    }

    /// Find a corresponding source map for the given address.
    pub fn find_source(&self, address: Address) -> Option<Arc<Source>> {
        self.item_by_address
            .get(&address)
            .and_then(|item| self.sources.get(item))
            .map(Arc::clone)
    }

    /// Register a runtime source.
    pub fn register_runtime_source(&mut self, item: String, source: Source) {
        self.runtime_sources.insert(item, Arc::new(source));
    }

    /// Find a corresponding runtime source map for the given address.
    pub fn find_runtime_source(&self, address: Address) -> Option<Arc<Source>> {
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

    /// Construct source information for the given code and source map.
    pub fn source(&self, bin: &str, source_map: &str) -> Result<Source, Error> {
        let source_map = SourceMap::parse(source_map)
            .map_err(|e| format!("failed to decode source map: {}", e))?;

        let offsets = self.decode_offsets(bin)
            .map_err(|e| format!("failed to decode offsets from bin: {}", e))?;

        Ok(Source {
            source_map,
            offsets,
        })
    }

    /// Decoded the given code into instruction offsets.
    pub fn decode_offsets(&self, code: &str) -> Result<HashMap<usize, usize>, Error> {
        // maps byte offset to instruction offset, to permit looking it up from a tracer.
        let mut out = HashMap::new();

        let mut n = 0;
        let mut offset = 0;

        out.insert(n, offset);

        let mut it = Decoder::new(code);

        while let Some(section) = it.next() {
            let section = section?;

            match section {
                Section::Instruction(_, _) => {
                    n += 1;
                    offset += 1;
                }
                Section::Push(_, push) => {
                    n += 1;
                    offset += 1;

                    match push {
                        Push::Unlinked(_) => {
                            // length of an unlinked section
                            n += 20;
                        }
                        Push::Bytes(bytes) => {
                            n += bytes.len();
                        }
                    }
                }
                Section::BadInstruction(_) => {
                    // causes an exception, but otherwise ignore
                    n += 1;
                    offset += 1;
                }
                Section::SwarmHash(..) => {
                    // ignore
                    continue;
                }
            }

            out.insert(n, offset);
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
    pub fn link(&self, code: &str) -> Result<Vec<u8>, Error> {
        let mut it = Decoder::new(code);
        let mut output = Vec::new();

        while let Some(section) = it.next() {
            let section = section?;

            let push = match section {
                Section::Instruction(b, _) => {
                    output.push(b);
                    continue;
                }
                Section::Push(b, push) => {
                    output.push(b);
                    push
                }
                Section::BadInstruction(b) => {
                    output.push(b);
                    continue;
                }
                Section::SwarmHash(bytes, _) => {
                    output.extend(bytes);
                    continue;
                }
            };

            let unlinked = match push {
                Push::Bytes(bytes) => {
                    output.extend(bytes);
                    continue;
                }
                Push::Unlinked(unlinked) => unlinked,
            };

            let (path, item) = decode_linked(unlinked)?;

            let address = match item {
                Some(item) => self.objects_by_item
                    .get(item)
                    .ok_or_else(|| Error::NoLinkerItem {
                        item: item.to_string(),
                    })?,
                None => self.objects_by_path
                    .get(path)
                    .ok_or_else(|| Error::NoLinkerPath {
                        path: path.to_string(),
                    })?,
            };

            output.extend(address.iter());
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

#[derive(Debug)]
pub enum Push<'a> {
    Bytes(Vec<u8>),
    Unlinked(&'a str),
}

#[derive(Debug)]
pub enum Section<'a> {
    /// A bad instruction.
    BadInstruction(u8),
    /// A regular instruction.
    Instruction(u8, parity_evm::Instruction),
    /// A push instruction.
    Push(u8, Push<'a>),
    /// Swarm hash as seen at end of contract.
    SwarmHash(Vec<u8>, Vec<u8>),
}

#[derive(Debug)]
pub struct Decoder<'a> {
    pos: usize,
    input: HexDecode<'a>,
}

impl<'a> Decoder<'a> {
    fn new(input: &'a str) -> Decoder<'a> {
        Decoder {
            pos: 0usize,
            input: HexDecode(input),
        }
    }
}

impl<'a> Iterator for Decoder<'a> {
    type Item = Result<Section<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let swarm_hash = match self.input.take_swarm_hash() {
            Ok(swarm_hash) => swarm_hash,
            Err(e) => return Some(Err(format!("{}: #{}", e, self.pos).into())),
        };

        if let Some((bytes, hash)) = swarm_hash {
            return Some(Ok(Section::SwarmHash(bytes, hash)));
        }

        let c = match self.input.next() {
            Some(c) => c,
            None => return None,
        };

        self.pos += 1;

        let c = match c {
            Ok(c) => c,
            Err(_) => return Some(Err(format!("bad hex: #{}", self.pos).into())),
        };

        let info = match parity_evm::Instruction::from_u8(c) {
            Some(info) => info,
            None => {
                return Some(Ok(Section::BadInstruction(c)));
            }
        };

        let bytes = match info.push_bytes() {
            Some(bytes) => bytes,
            None => {
                return Some(Ok(Section::Instruction(c, info)));
            }
        };

        let bytes = match self.input.take_raw(bytes) {
            Some(bytes) => bytes,
            None => {
                return Some(Err(
                    format!("not enough input for push: #{}", self.pos).into()
                ))
            }
        };

        // unlinked section.
        if bytes.len() == 40 {
            if &bytes[0..2] == "__" {
                return Some(Ok(Section::Push(c, Push::Unlinked(bytes))));
            };
        }

        let mut decoder = HexDecode(bytes);
        let mut out = Vec::new();

        while let Some(b) = decoder.next() {
            let b = match b {
                Ok(b) => b,
                Err(_) => return Some(Err(format!("bad hex in section: #{}", self.pos).into())),
            };

            out.push(b);
        }

        return Some(Ok(Section::Push(c, Push::Bytes(out))));
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct BadHex;

#[derive(Debug, Clone)]
pub struct HexDecode<'a>(&'a str);

macro_rules! decode_hex_digit {
    ($source:expr) => {{
        let __d = match $source.chars().next() {
            Some(__d) => __d,
            None => return None,
        };

        if __d.len_utf8() > 1 {
            $source = "";
            return Some(Err(BadHex));
        }

        let __d = HEX[__d as usize];

        if __d < 0 {
            $source = "";
            return Some(Err(BadHex));
        }

        $source = &$source[1..];
        __d as u8
    }};
}

impl<'a> HexDecode<'a> {
    /// Take a slice of bytes.
    fn take_raw(&mut self, len: usize) -> Option<&'a str> {
        let len = len * 2;

        if self.0.len() < len {
            return None;
        }

        if !self.0.is_char_boundary(len) {
            return None;
        }

        let (out, rest) = self.0.split_at(len);
        self.0 = rest;
        Some(out)
    }

    /// Try to take swarm hash, if present.
    fn take_swarm_hash(&mut self) -> Result<Option<(Vec<u8>, Vec<u8>)>, Error> {
        if self.0.len() != 86 {
            return Ok(None);
        }

        if !self.0.starts_with("a165627a7a72305820") {
            return Ok(None);
        }

        if !self.0.ends_with("0029") {
            return Ok(None);
        }

        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend(b"\xa1\x65\x62\x7a\x7a\x72\x30\x58\x20");

        let hash = &self.0[18..];
        let hash = &hash[..64];

        let mut decoder = HexDecode(hash);
        let mut hash = Vec::new();

        while let Some(b) = decoder.next() {
            let b = match b {
                Ok(b) => b,
                Err(_) => return Err("bad hex in swarm hash".into()),
            };

            hash.push(b);
        }

        bytes.extend(hash.iter().cloned());
        bytes.extend(b"\x00\x29");

        self.0 = "";
        Ok(Some((bytes, hash)))
    }
}

impl<'a> Iterator for HexDecode<'a> {
    type Item = Result<u8, BadHex>;

    fn next(&mut self) -> Option<Self::Item> {
        let a = decode_hex_digit!(self.0) << 4;
        let b = decode_hex_digit!(self.0);
        return Some(Ok(a + b));
    }
}

#[cfg(test)]
mod tests {
    use super::HexDecode;
    use super::Linker;

    extern crate hex;

    #[test]
    fn test_linker() {
        let linker = Linker::new();

        let a = hex::decode("608060405234801561001057600080fd5b").expect("bad hex decode");

        let b = linker
            .link("608060405234801561001057600080fd5b")
            .expect("bad link decode");

        linker.link("FF").expect("bad link decode");
        assert_eq!(a, b);
    }

    #[test]
    fn test_contract_a_linker() {
        let mut linker = Linker::new();
        linker.register_item("SimpleLib".to_string(), 0x342a.into());

        let out = linker
            .link(include_str!("tests/a.bin").trim())
            .expect("bad link decode");

        // already linked should have no effect.
        let linked = linker
            .link(include_str!("tests/linked_a.bin").trim())
            .expect("bad link decode");

        assert_eq!(linked, out);
    }

    #[test]
    fn test_decode_runtime_simple() {
        let linker = Linker::new();

        let _decoded = linker
            .decode_offsets(include_str!("tests/runtime_simple.bin").trim())
            .expect("bad decode");
    }

    #[test]
    fn test_decode_runtime_big() {
        let linker = Linker::new();

        let _decoded = linker
            .decode_offsets(include_str!("tests/runtime_big.bin").trim())
            .expect("bad decode");
    }

    #[test]
    fn test_hex_decode() {
        let decoded = HexDecode("00112233445566778899").collect::<Vec<_>>();

        assert_eq!(
            vec![
                Ok(0x00),
                Ok(0x11),
                Ok(0x22),
                Ok(0x33),
                Ok(0x44),
                Ok(0x55),
                Ok(0x66),
                Ok(0x77),
                Ok(0x88),
                Ok(0x99),
            ],
            decoded
        );
    }
}