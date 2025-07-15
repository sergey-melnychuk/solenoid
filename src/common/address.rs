use primitive_types::U256;

#[derive(Clone, Copy, Default, Hash, Eq, PartialEq)]
pub struct Address(pub [u8; 20]);

impl Address {
    pub fn zero() -> Self {
        Self([0u8; 20])
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

impl From<&Address> for U256 {
    fn from(value: &Address) -> Self {
        let mut bytes = [0u8; 32];
        bytes[12..].copy_from_slice(&value.0);
        U256::from_big_endian(&bytes)
    }
}

impl From<&U256> for Address {
    fn from(value: &U256) -> Self {
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
        if value.len() != 40 {
            return Err(crate::common::error::Error::InvalidAddress);
        }
        let mut bytes = [0u8; 20];
        hex::decode_to_slice(value.trim_start_matches("0x"), &mut bytes)
            .map_err(|_| crate::common::error::Error::InvalidAddress)?;
        Ok(Address(bytes))
    }
}
