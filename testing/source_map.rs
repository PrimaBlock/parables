use failure::Error;

macro_rules! parse_u32 {
    ($p:expr, $var:expr) => {
        match $p.next() {
            Some("") | None => $var,
            Some("-1") => {
                $var = None;
                None
            }
            Some(string) => {
                let value = u32::from_str(string)
                    .map_err(|e| format_err!("failed to decode u32: {}: {}", string, e))?;
                $var = Some(value);
                Some(value)
            }
        }
    };
}

macro_rules! parse_op {
    ($p:expr, $var:expr, { $($m:expr => $v:expr,)* }) => {
        match $p.next() {
            $(Some($m) => {
                $var = Some($v);
                Some($v)
            })*
            Some(op) => return Err(format_err!("bad operation: {}", op)),
            None => $var.clone(),
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    // -
    None,
    // i
    Input,
    // o
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    pub start: u32,
    pub length: u32,
    pub file_index: Option<u32>,
    pub operation: Operation,
}

/// A parsed source map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    mappings: Vec<Mapping>,
}

impl SourceMap {
    /// Parse the given source map.
    pub fn parse(input: &str) -> Result<SourceMap, Error> {
        use std::str::FromStr;

        let mut mappings = Vec::new();

        let mut segments = input.split(";");

        let mut start: Option<u32> = None;
        let mut length: Option<u32> = None;
        let mut file_index: Option<u32> = None;
        let mut operation: Option<Operation> = None;

        while let Some(segment) = segments.next() {
            let mut parts = segment.split(":");

            let start = parse_u32!(parts, start).ok_or_else(|| format_err!("missing start byte"))?;
            let length = parse_u32!(parts, length).ok_or_else(|| format_err!("missing length"))?;
            let file_index = parse_u32!(parts, file_index);

            let operation = parse_op!(parts, operation, {
                "i" => Operation::Input,
                "o" => Operation::Output,
                "-" => Operation::None,
            }).unwrap_or(Operation::None);

            mappings.push(Mapping {
                start,
                length,
                file_index,
                operation,
            });
        }

        Ok(SourceMap { mappings })
    }

    /// Find the mapping for a given program counter.
    pub fn find_mapping(&self, pc: usize) -> Option<&Mapping> {
        self.mappings.get(pc)
    }
}

#[cfg(test)]
mod tests {
    use super::SourceMap;

    #[test]
    fn test_parse() {
        let source_map = SourceMap::parse("25:111:1:-;;132:2:-1;166:7;155:9;146:7;137:37;252:7;246:14;243:1;238:23;232:4;229:33;270:1;265:20;;;;222:63;;265:20;274:9;222:63;;298:9;295:1;288:20;328:4;319:7;311:22;352:7;343;336:24").unwrap();

        println!("{:?}", source_map);
    }
}
