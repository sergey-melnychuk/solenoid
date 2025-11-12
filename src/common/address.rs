use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::common::{decode, hash::keccak256, word::Word};

#[derive(Clone, Copy, Default, Hash, Eq, PartialEq)]
pub struct Address(pub [u8; 20]);

impl Address {
    pub fn zero() -> Self {
        Self([0u8; 20])
    }

    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|byte| byte == &0)
    }

    pub fn create2(&self, salt: &Word, code: &[u8]) -> Address {
        // (See: https://www.evm.codes/?fork=cancun#f5)
        // initialisation_code = memory[offset:offset+size]
        // address = keccak256(0xff + sender_address + salt + keccak256(initialisation_code))[12:]
        let mut buffer = Vec::with_capacity(1 + 20 + 32 + 32);
        buffer.push(0xffu8);
        buffer.extend_from_slice(&self.0); // Use 'this' (current contract), not call.from
        buffer.extend_from_slice(&salt.into_bytes());
        buffer.extend_from_slice(&keccak256(code)); // Use raw bytecode from memory
        let mut hash = keccak256(&buffer);
        hash[0..12].copy_from_slice(&[0u8; 12]);
        Address::from(&Word::from_bytes(&hash))
    }

    pub fn create(&self, nonce: Word) -> Address {
        // https://www.evm.codes/?fork=cancun#55
        // address = keccak256(rlp([sender_address,sender_nonce]))[12:]
        // Breakdown:
        //   0xd8   = List prefix (0xc0 + 24 bytes total length)
        //   0x94   = Address prefix (0x80 + 20 bytes)
        //   5bc1c1942f2333acb9ce156525bc079fad983f13 = Factory address (20 bytes)
        //   0x82   = Nonce prefix (0x80 + 2 bytes)
        //   065b   = Nonce value 1627 in big-endian (2 bytes)
        let address_bytes = self.0.to_vec();
        let nonce_bytes = nonce
            .into_bytes()
            .into_iter()
            .skip_while(|byte| byte == &0)
            .collect::<Vec<_>>();

        let mut buffer = Vec::new();

        // Calculate the total payload size for the list
        // Payload includes: address_prefix (1) + address (20) + nonce_encoding
        let nonce_encoding_len =
            if nonce_bytes.is_empty() || nonce_bytes.len() == 1 && nonce_bytes[0] < 0x80 {
                1 // 0x80 for integer 0 OR single byte for integers 1-127
            } else {
                1 + nonce_bytes.len() // prefix + bytes for integers > 127
            };
        let payload_len = 1 + address_bytes.len() + nonce_encoding_len;

        buffer.push(0xc0u8 + payload_len as u8);
        buffer.push(0x80u8 + address_bytes.len() as u8);
        buffer.extend_from_slice(&address_bytes);

        // RLP encode the nonce according to spec
        if nonce_bytes.is_empty() {
            // Integer 0 is encoded as 0x80 (empty byte string)
            buffer.push(0x80u8);
        } else if nonce_bytes.len() == 1 && nonce_bytes[0] < 0x80 {
            // Integers 1-127 are encoded as a single byte
            buffer.push(nonce_bytes[0]);
        } else {
            // Integers > 127 are encoded with prefix
            buffer.push(0x80u8 + nonce_bytes.len() as u8);
            buffer.extend_from_slice(&nonce_bytes);
        }

        let hash = super::hash::keccak256(&buffer);
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[12..32]);
        Address(addr)
    }

    pub fn as_word(&self) -> Word {
        Word::from_bytes(&self.0)
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl std::fmt::Debug for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Address(0x{})", hex::encode(self.0))
    }
}

impl From<&Address> for Word {
    fn from(value: &Address) -> Self {
        let mut bytes = [0u8; 32];
        bytes[12..].copy_from_slice(&value.0);
        Word::from_bytes(&bytes)
    }
}

impl From<&Word> for Address {
    fn from(value: &Word) -> Self {
        let bytes: [u8; 32] = value.into_bytes();
        let mut ret = Address::default();
        ret.0[..].copy_from_slice(&bytes[12..]);
        ret
    }
}

impl From<[u8; 20]> for Address {
    fn from(value: [u8; 20]) -> Self {
        Self(value)
    }
}

impl TryFrom<&[u8]> for Address {
    type Error = crate::common::error::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.len() != 20 {
            return Err(crate::common::error::Error::InvalidAddress);
        }
        let mut bytes = [0u8; 20];
        bytes.copy_from_slice(value);
        Ok(Address(bytes))
    }
}

impl TryFrom<&str> for Address {
    type Error = crate::common::error::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.len() != 40 && value.len() != 42 {
            return Err(crate::common::error::Error::InvalidAddress);
        }
        let mut bytes = [0u8; 20];
        hex::decode_to_slice(value.trim_start_matches("0x"), &mut bytes)
            .map_err(|_| crate::common::error::Error::InvalidAddress)?;
        Ok(Address(bytes))
    }
}

impl Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex = hex::encode(self.0);
        let hex = format!("0x{hex}");
        serializer.serialize_str(&hex)
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Address, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        let hex: String = Deserialize::deserialize(deserializer)?;
        let hex = hex.trim_start_matches("0x");
        if hex.len() != 40 {
            return Err(D::Error::invalid_value(
                serde::de::Unexpected::Str(hex),
                &"Invalid hex length",
            ));
        }
        Ok(addr(hex))
    }
}

pub const fn addr(s: &str) -> Address {
    Address(decode(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_address() {
        assert_eq!(
            addr("0x5bc1c1942f2333acb9ce156525bc079fad983f13")
                .create(Word::from_hex("0x065b").unwrap()),
            addr("0xe77afefd5b7beb79d1843e65a0fd54963abc742f")
        );
    }
}
