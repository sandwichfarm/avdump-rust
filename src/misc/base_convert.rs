//! Arbitrary-radix rendering of byte strings (big-endian, big-integer semantics).
//!
//! Mirrors `BitConverterEx.ToBase`: the digest is interpreted as one unsigned big-endian integer
//! and printed with the given digit alphabet, left-padded with the zero digit to the number of
//! digits a value of that byte length can need.

/// Digit alphabets addressable from the `Hash-<Name>-<Base>-<Case>` placeholder.
pub fn digits_for(base: &str) -> Option<&'static str> {
    Some(match base {
        "2" => "01",
        "4" => "0123",
        "8" => "01234567",
        "10" => "0123456789",
        "16" => "0123456789ABCDEF",
        "32Hex" => "0123456789ABCDEFGHIJKLMNOPQRSTUV",
        "32Z" => "0123456789ABCDEFGHJKMNPQRSTVWXYZ",
        "32" => "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567",
        "36" => "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        "62" => "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
        "64" => "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ+/",
        _ => return None,
    })
}

/// Names of all supported bases, in help-menu order.
pub const BASE_NAMES: &[&str] = &["2", "4", "8", "10", "16", "32", "32Hex", "32Z", "36", "62", "64"];

/// Render `value` (big-endian) in the radix given by `digits`.
pub fn to_base(value: &[u8], digits: &str) -> String {
    let alphabet: Vec<char> = digits.chars().collect();
    let radix = alphabet.len() as u32;
    assert!(radix >= 2, "expected at least two digits");

    let bits = 8.0 * value.len() as f64;
    let pad = (bits / (radix as f64).log2()).ceil() as usize;

    // Repeated long division of the big-endian magnitude.
    let mut mag: Vec<u8> = value.iter().copied().skip_while(|b| *b == 0).collect();
    let mut out: Vec<char> = Vec::with_capacity(pad.max(1));
    if mag.is_empty() {
        out.push(alphabet[0]);
    }
    while !mag.is_empty() {
        let mut rem: u32 = 0;
        let mut next = Vec::with_capacity(mag.len());
        for &b in &mag {
            let cur = (rem << 8) | b as u32;
            let q = cur / radix;
            rem = cur % radix;
            if !(next.is_empty() && q == 0) {
                next.push(q as u8);
            }
        }
        out.push(alphabet[rem as usize]);
        mag = next;
    }
    while out.len() < pad {
        out.push(alphabet[0]);
    }
    out.iter().rev().collect()
}

/// RFC 4648 base32 (no padding), as used by DC++ / rhash for TTH values.
pub fn rfc4648_base32(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &b in data {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 0x1F) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(to_base(&[0xCB, 0xF4, 0x39, 0x26], digits_for("16").unwrap()), "CBF43926");
        assert_eq!(to_base(&[0, 0, 0, 1], digits_for("16").unwrap()), "00000001");
        assert_eq!(to_base(&[0, 0, 0, 0], digits_for("16").unwrap()), "00000000");
    }

    #[test]
    fn decimal_and_binary() {
        assert_eq!(to_base(&[0xFF], digits_for("10").unwrap()), "255");
        assert_eq!(to_base(&[0x01, 0x00], digits_for("10").unwrap()), "00256");
        assert_eq!(to_base(&[0b1010_0101], digits_for("2").unwrap()), "10100101");
        assert_eq!(to_base(&[0xFF, 0xFF], digits_for("36").unwrap()), "1EKF");
    }

    #[test]
    fn base32_rfc() {
        assert_eq!(rfc4648_base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(rfc4648_base32(b""), "");
    }
}
