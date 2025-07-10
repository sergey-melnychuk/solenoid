use primitive_types::U256;

#[derive(Debug, Default)]
pub struct Address(pub [u8; 20]);

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
