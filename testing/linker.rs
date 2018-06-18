use error::{Error, Result};
use ethereum_types::Address;
use std::collections::HashMap;

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
pub struct Linker {
    /// Known linkable objects by item.
    objects_by_item: HashMap<String, Address>,
    /// Known linkable objects by path.
    objects_by_path: HashMap<String, Address>,
}

impl Linker {
    /// Construct a new linker.
    pub fn new() -> Self {
        Self {
            objects_by_item: HashMap::new(),
            objects_by_path: HashMap::new(),
        }
    }

    /// Register an address for an item.
    pub fn register_item(&mut self, item: String, address: Address) {
        self.objects_by_item.insert(item, address);
    }

    /// Register an address for a path.
    pub fn register_path(&mut self, path: String, address: Address) {
        self.objects_by_path.insert(path, address);
    }

    /// Decode and link the given bytecode.
    ///
    /// The bytecode is represented in ascii, where each byte corresponds to a hex character.
    ///
    /// Entries to be linked are designated with two underscores `__`, these should be replaced
    /// with an address corresponding to the linked object.
    ///
    /// All other entries should be left preserved.
    pub fn link(&self, mut code: &[u8]) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        let mut n = 0usize;

        // read input in pairs.
        while code.len() >= 2 {
            match &code[..2] {
                // section to link
                &[b'_', b'_'] => {
                    if code.len() < 40 {
                        bail!(
                            "expected link section at position {}, but remaining code is too small",
                            n
                        );
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
                    bail!("bad input at position {}, expect `__` or two hex digits", n);
                }
            }
        }

        return Ok(output);

        /// Decode a single 40-byte linking section.
        ///
        /// Generally has the structure `<path>:<item>`, where `<item>` is optional since it might
        /// not fit within the section.
        fn decode_linked(chunk: &[u8]) -> Result<(&str, Option<&str>)> {
            let chunk = ::std::str::from_utf8(chunk)?;
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
            .link(b"01234567789abcdefABCDEFF")
            .expect("bad link decode");

        linker.link(b"FF").expect("bad link decode");
        assert_eq!(a, b);
    }

    #[test]
    fn test_linker_against_contract_a() {
        let mut linker = Linker::new();
        linker.register_item("SimpleLib".to_string(), 0x342a.into());

        let out = linker
            .link(include_bytes!("tests/a.bin"))
            .expect("bad link decode");

        // already linked should have no effect.
        let linked = linker
            .link(include_bytes!("tests/linked_a.bin"))
            .expect("bad link decode");

        assert_eq!(linked, out);
    }
}
