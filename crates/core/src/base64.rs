//! Base64 estándar (RFC 4648, con relleno), sin dependencias.

const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                let idx = usize::try_from((n >> (18 - 6 * i)) & 63).unwrap_or(0);
                out.push(char::from(T[idx]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Descodifica base64 estándar sin espacios, con o sin el relleno (`=`) del final. `None` si
/// lleva otra cosa o le sobra un carácter.
pub fn decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=').as_bytes();
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3 + 2);
    for chunk in s.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            let v = T.iter().position(|t| t == c)?;
            n |= u32::try_from(v).ok()? << (18 - 6 * i);
        }
        out.extend_from_slice(&n.to_be_bytes()[1..chunk.len()]);
    }
    Some(out)
}

/// Base64 en líneas de `width` caracteres, cada una terminada en `\n` (como `chunk_split`).
pub fn encode_wrapped(bytes: &[u8], width: usize) -> String {
    let s = encode(bytes);
    let mut out = String::with_capacity(s.len() + s.len() / width.max(1) + 1);
    for chunk in s.as_bytes().chunks(width.max(1)) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4648() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(i.as_bytes()), o);
        }
        assert_eq!(encode_wrapped(b"foobar", 4), "Zm9v\nYmFy\n");
        assert_eq!(encode_wrapped(b"fo", 76), "Zm8=\n");
    }

    #[test]
    fn descodifica_con_y_sin_relleno() {
        for (o, i) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(decode(i).as_deref(), Some(o.as_bytes()));
            assert_eq!(
                decode(i.trim_end_matches('=')).as_deref(),
                Some(o.as_bytes())
            );
        }
        let todo: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&todo)), Some(todo));
        // Con otra cosa o con un carácter de más, no.
        for mal in ["Zm9v YmFy", "Zm9v-", "Zm9vY", "Zg=a"] {
            assert_eq!(decode(mal), None, "{mal}");
        }
    }
}
