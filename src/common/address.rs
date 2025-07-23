use crate::common::Word;

#[derive(Clone, Copy, Default, Hash, Eq, PartialEq)]
pub struct Address(pub [u8; 20]);

impl Address {
    pub fn zero() -> Self {
        Self([0u8; 20])
    }

    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|byte| byte == &0)
    }

    pub fn of_smart_contract(&self, nonce: Word) -> Address {
        // https://www.evm.codes/?fork=cancun#55
        // address = keccak256(rlp([sender_address,sender_nonce]))[12:]
        let a: Word = self.into();
        let a: [u8; 32] = a.to_big_endian();
        let b: [u8; 32] = nonce.to_big_endian();
        let mut buffer = [0u8; 64];
        buffer[0..32].copy_from_slice(&a);
        buffer[32..].copy_from_slice(&b);
        let hash = super::hash::keccak256(&buffer);
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[12..32]);
        Address(addr)
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
        Word::from_big_endian(&bytes)
    }
}

impl From<&Word> for Address {
    fn from(value: &Word) -> Self {
        let bytes: [u8; 32] = value.to_big_endian();
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

#[cfg(test)]
mod tests {
    use crate::common::addr;

    use super::*;

    #[test]
    fn test_create_address() {
        assert_eq!(
            addr("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512").of_smart_contract(Word::zero()),
            addr("0xc80a141ce8a5b73371043cba5cee40437975bb37")
        );
        assert_eq!(
            addr("0xc80a141ce8a5b73371043cba5cee40437975bb37").of_smart_contract(Word::zero()),
            addr("0xc26297fdd7b51a5c8c4ffe76f06af56680e2b552")
        );
    }
}
