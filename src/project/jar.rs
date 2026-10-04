//! Minimal jar (zip) reading: the entry names of the central directory.

use std::path::Path;

fn u16le(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]) as usize)
}

fn u32le(b: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]) as usize)
}

/// The names of the entries of the zip archive at `path`.
pub fn entry_names(path: &Path) -> Option<Vec<String>> {
    let data = std::fs::read(path).ok()?;
    // End of central directory record: 22 bytes + comment (≤ 64 KiB).
    let min = data.len().saturating_sub(22 + 65535);
    let mut eocd = None;
    let mut i = data.len().checked_sub(22)?;
    loop {
        if data[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            eocd = Some(i);
            break;
        }
        if i == min || i == 0 {
            break;
        }
        i -= 1;
    }
    let eocd = eocd?;
    let count = u16le(&data, eocd + 10)?;
    let mut off = u32le(&data, eocd + 16)?;
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        if data.get(off..off + 4)? != [0x50, 0x4b, 0x01, 0x02] {
            break;
        }
        let name_len = u16le(&data, off + 28)?;
        let extra_len = u16le(&data, off + 30)?;
        let comment_len = u16le(&data, off + 32)?;
        let name = data.get(off + 46..off + 46 + name_len)?;
        names.push(String::from_utf8_lossy(name).into_owned());
        off += 46 + name_len + extra_len + comment_len;
    }
    Some(names)
}

/// Whether the jar at `path` holds the class `fqn` (`a.b.C`).
pub fn has_class(path: &Path, fqn: &str) -> bool {
    let entry = format!("{}.class", fqn.replace('.', "/"));
    entry_names(path).is_some_and(|names| names.iter().any(|n| *n == entry))
}
