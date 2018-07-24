use error::Error;
use std::io::Read;

const NL: u8 = '\n' as u8;

/// Find the line for a given span.
pub fn find_line(
    reader: impl Read,
    span: (usize, usize),
) -> Result<(String, usize, (usize, usize)), Error> {
    let mut line = 1usize;
    let mut current = 0usize;
    let mut buffer: Vec<u8> = Vec::new();

    let start = span.0;
    let end = span.1;

    let mut it = reader.bytes().peekable();
    let mut read = 0usize;

    while let Some(b) = it.next() {
        let b = b.map_err(|e| format!("failed to read byte: {}", e))?;
        read += 1;

        match b {
            NL => {}
            _ => {
                buffer.push(b);
                continue;
            }
        }

        let start_of_line = current;
        current += read;

        if current > start {
            let buffer = String::from_utf8(buffer).map_err(|e| format!("bad utf-8 line: {}", e))?;
            let buffer = buffer.trim().to_string();

            let end = ::std::cmp::min(end, current);
            let range = (start - start_of_line, end - start_of_line);
            return Ok((buffer, line, range));
        }

        read = 0usize;
        line += 1;
        buffer.clear();
    }

    Err("bad file position".into())
}