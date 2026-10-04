//! `SerialVersionHashOperationCore.calculateSerialVersionId`: the default
//! serialization hash of a compiled class file.

/// Minimal class file view (`IClassFileReader`).
struct ClassFile {
    name: String,
    access: u16,
    interfaces: Vec<String>,
    fields: Vec<(String, u16, String)>,
    methods: Vec<(String, u16, String)>,
    inner_flags: Option<u16>,
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

fn parse(bytes: &[u8]) -> Option<ClassFile> {
    if u32_at(bytes, 0)? != 0xCAFE_BABE {
        return None;
    }
    let cp_count = u16_at(bytes, 8)? as usize;
    // Constant pool: utf8 strings and class name indices.
    let mut utf8: Vec<Option<String>> = vec![None; cp_count];
    let mut class_name_index: Vec<Option<u16>> = vec![None; cp_count];
    let mut i = 10;
    let mut idx = 1;
    while idx < cp_count {
        let tag = *bytes.get(i)?;
        match tag {
            1 => {
                let len = u16_at(bytes, i + 1)? as usize;
                let raw = bytes.get(i + 3..i + 3 + len)?;
                utf8[idx] = Some(decode_modified_utf8(raw));
                i += 3 + len;
            }
            7 => {
                class_name_index[idx] = Some(u16_at(bytes, i + 1)?);
                i += 3;
            }
            8 | 16 | 19 | 20 => i += 3,
            15 => i += 4,
            3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => i += 5,
            5 | 6 => {
                i += 9;
                idx += 1;
            }
            _ => return None,
        }
        idx += 1;
    }
    let class_name = |ci: u16| -> Option<String> { utf8.get(class_name_index.get(ci as usize)?.as_ref().copied()? as usize)?.clone() };
    let access = u16_at(bytes, i)?;
    let this_class = u16_at(bytes, i + 2)?;
    let name = class_name(this_class)?;
    i += 6;
    let n_interfaces = u16_at(bytes, i)? as usize;
    i += 2;
    let mut interfaces = Vec::new();
    for _ in 0..n_interfaces {
        interfaces.push(class_name(u16_at(bytes, i)?)?);
        i += 2;
    }
    let mut read_members = |i: &mut usize| -> Option<Vec<(String, u16, String)>> {
        let n = u16_at(bytes, *i)? as usize;
        *i += 2;
        let mut out = Vec::new();
        for _ in 0..n {
            let flags = u16_at(bytes, *i)?;
            let name = utf8.get(u16_at(bytes, *i + 2)? as usize)?.clone()?;
            let desc = utf8.get(u16_at(bytes, *i + 4)? as usize)?.clone()?;
            let attrs = u16_at(bytes, *i + 6)? as usize;
            *i += 8;
            for _ in 0..attrs {
                let len = u32_at(bytes, *i + 2)? as usize;
                *i += 6 + len;
            }
            out.push((name, flags, desc));
        }
        Some(out)
    };
    let fields = read_members(&mut i)?;
    let methods = read_members(&mut i)?;
    // Class attributes: InnerClasses.
    let mut inner_flags = None;
    let n_attrs = u16_at(bytes, i)? as usize;
    i += 2;
    for _ in 0..n_attrs {
        let attr_name = utf8.get(u16_at(bytes, i)? as usize)?.clone()?;
        let len = u32_at(bytes, i + 2)? as usize;
        if attr_name == "InnerClasses" {
            let n = u16_at(bytes, i + 6)? as usize;
            let mut j = i + 8;
            for _ in 0..n {
                let inner = u16_at(bytes, j)?;
                let flags = u16_at(bytes, j + 6)?;
                if inner != 0 && class_name(inner).as_deref() == Some(name.as_str()) && inner_flags.is_none() {
                    inner_flags = Some(flags);
                }
                j += 8;
            }
        }
        i += 6 + len;
    }
    Some(ClassFile { name, access, interfaces, fields, methods, inner_flags })
}

fn decode_modified_utf8(raw: &[u8]) -> String {
    let mut units: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let a = raw[i] as u16;
        if a & 0x80 == 0 {
            units.push(a);
            i += 1;
        } else if a & 0xE0 == 0xC0 && i + 1 < raw.len() {
            units.push(((a & 0x1F) << 6) | (raw[i + 1] as u16 & 0x3F));
            i += 2;
        } else if i + 2 < raw.len() {
            units.push(((a & 0x0F) << 12) | ((raw[i + 1] as u16 & 0x3F) << 6) | (raw[i + 2] as u16 & 0x3F));
            i += 3;
        } else {
            i += 1;
        }
    }
    String::from_utf16_lossy(&units)
}

/// `DataOutputStream.writeUTF`.
fn write_utf(out: &mut Vec<u8>, s: &str) {
    let mut bytes = Vec::new();
    for u in s.encode_utf16() {
        if (1..=0x7F).contains(&u) {
            bytes.push(u as u8);
        } else if u <= 0x7FF {
            bytes.push((0xC0 | ((u >> 6) & 0x1F)) as u8);
            bytes.push((0x80 | (u & 0x3F)) as u8);
        } else {
            bytes.push((0xE0 | ((u >> 12) & 0x0F)) as u8);
            bytes.push((0x80 | ((u >> 6) & 0x3F)) as u8);
            bytes.push((0x80 | (u & 0x3F)) as u8);
        }
    }
    out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    out.extend(bytes);
}

/// `CharOperation.compareTo`.
fn char_compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `calculateSerialVersionId(cfReader)`.
pub fn serial_version_id(class_bytes: &[u8]) -> Option<i64> {
    let cf = parse(class_bytes)?;
    let mut out = Vec::new();
    write_utf(&mut out, &cf.name.replace('/', "."));
    let modifiers = cf.inner_flags.unwrap_or(cf.access) as i32 & 1553;
    out.extend_from_slice(&modifiers.to_be_bytes());
    let mut interfaces = cf.interfaces.clone();
    interfaces.sort_by(|a, b| char_compare(a, b));
    for i in &interfaces {
        write_utf(&mut out, &i.replace('/', "."));
    }
    let mut fields = cf.fields.clone();
    fields.sort_by(|a, b| char_compare(&a.0, &b.0));
    for (name, flags, desc) in &fields {
        let flags = *flags as i32;
        let private = flags & 0x2 != 0;
        let is_static = flags & 0x8 != 0;
        let transient = flags & 0x80 != 0;
        if !private || !is_static && !transient {
            write_utf(&mut out, name);
            out.extend_from_slice(&(flags & 223).to_be_bytes());
            write_utf(&mut out, desc);
        }
    }
    if cf.methods.iter().any(|m| m.0 == "<clinit>") {
        write_utf(&mut out, "<clinit>");
        out.extend_from_slice(&8i32.to_be_bytes());
        write_utf(&mut out, "()V");
    }
    let mut methods = cf.methods.clone();
    methods.sort_by(|a, b| {
        let (ca, cb) = (a.0 == "<init>", b.0 == "<init>");
        if ca != cb {
            return if ca { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        if ca {
            return std::cmp::Ordering::Equal;
        }
        char_compare(&a.0, &b.0).then_with(|| char_compare(&a.2, &b.2))
    });
    for (name, flags, desc) in &methods {
        let flags = *flags as i32;
        if flags & 0x2 == 0 && name != "<clinit>" {
            write_utf(&mut out, name);
            out.extend_from_slice(&(flags & 3391).to_be_bytes());
            write_utf(&mut out, &desc.replace('/', "."));
        }
    }
    let sha = sha1(&out);
    let mut hash: i64 = 0;
    for i in (0..8).rev() {
        hash = (hash << 8) | sha[i] as i64;
    }
    Some(hash)
}

/// Standard base64 decoding (`Base64.getEncoder()` output).
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\n' | b'\r' => continue,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// SHA-1 (FIPS 180-1).
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn sha1_known_vector() {
        let d = super::sha1(b"abc");
        let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "a9993e364706816aba3e25717850c26c9cd0d89d");
    }
}
