use failure::Error;
use std::io::Read;

const NL: u8 = '\n' as u8;

/// Find the line for a given span.
pub fn find_line(reader: impl Read, span: (usize, usize)) -> Result<(Vec<String>, usize), Error> {
    let mut out_line = None;
    let mut line = 0usize;
    let mut current = 0usize;
    let mut buf: Vec<u8> = Vec::new();

    let start = span.0;
    let end = span.1;

    let mut it = reader.bytes();

    let mut lines = Vec::new();

    while let Some(b) = it.next() {
        let b = b.map_err(|e| format_err!("failed to read byte: {}", e))?;

        match b {
            NL => {}
            _ => {
                buf.push(b);
                continue;
            }
        }

        current += buf.len() + 1usize;

        if current > start {
            lines.push(
                ::std::str::from_utf8(&buf)
                    .map_err(|e| format_err!("bad utf-8 line: {}", e))?
                    .to_string(),
            );

            if out_line.is_none() {
                out_line = Some(line);
            }
        }

        if current >= end {
            return Ok((lines, out_line.unwrap_or(0usize)));
        }

        line += 1;
        buf.clear();
    }

    lines.push(
        ::std::str::from_utf8(&buf)
            .map_err(|e| format_err!("bad utf-8 line: {}", e))?
            .to_string(),
    );

    Ok((lines, out_line.unwrap_or(0usize)))
}
