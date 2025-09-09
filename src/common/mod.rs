use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub mod account;
pub mod address;
pub mod call;
pub mod error;
pub mod hash;
pub mod word;

#[derive(Clone)]
pub struct Hex(Vec<u8>);

impl AsRef<[u8]> for Hex {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for Hex {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl<const N: usize> From<[u8; N]> for Hex {
    fn from(value: [u8; N]) -> Self {
        Self(value.to_vec())
    }
}

impl std::fmt::Debug for Hex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hex = hex::encode(&self.0);
        f.write_str(&hex)
    }
}

impl std::fmt::Display for Hex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hex = hex::encode(&self.0);
        f.write_str(&hex)
    }
}

impl Serialize for Hex {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex = format!("0x{}", hex::encode(&self.0));
        serializer.serialize_str(&hex)
    }
}

impl<'de> Deserialize<'de> for Hex {
    fn deserialize<D>(deserializer: D) -> Result<Hex, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        let hex: String = Deserialize::deserialize(deserializer)?;
        let bin = hex::decode(hex.trim_start_matches("0x")).map_err(|_| {
            D::Error::invalid_value(serde::de::Unexpected::Str(&hex), &"Invalid hex string")
        })?;
        Ok(Hex(bin))
    }
}

const fn decode<const N: usize>(s: &str) -> [u8; N] {
    let chars = s.as_bytes();
    let mut bytes = [0u8; N];
    if chars.is_empty() {
        return bytes;
    }

    let parity = chars.len() % 2;
    let skip = if chars[0] == b'0' && chars.len() > 1 && chars[1] == b'x' {
        2
    } else {
        0
    };

    if chars.len() - skip > N * 2 {
        panic!("Value too large");
    }

    let mut chr_idx = chars.len();
    let mut bin_idx = N;
    while chr_idx > skip {
        let chr = chars[chr_idx - 1];
        let chr = match chr {
            b'0'..=b'9' => chr - b'0',
            b'a'..=b'f' => chr - b'a' + 10,
            b'A'..=b'F' => chr - b'A' + 10,
            _ => panic!("Invalid hex char"),
        };

        if chr_idx % 2 == parity {
            bytes[bin_idx - 1] = chr;
        } else {
            bytes[bin_idx - 1] += chr << 4;
            bin_idx -= 1;
        }

        chr_idx -= 1;
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(s: &str) {
        let hex = hex::decode(s.trim_start_matches("0x")).expect("hex");
        assert_eq!(&decode::<20>(s), &hex[..], "{s}");
    }

    #[test]
    fn test_decode() {
        assert_eq!(
            decode("123456789abcdef"),
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        );
        assert_eq!(
            decode("0x123456789abcdef"),
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        );
        assert_eq!(
            decode("0123456789abcdef"),
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        );
        assert_eq!(
            decode("0x0123456789abcdef"),
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        );
        check("0xc80a141ce8a5b73371043cba5cee40437975bb37");
    }
}
