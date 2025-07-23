use crate::common::{Word, address::Address};

#[derive(Clone, Debug, Default)]
pub struct Call {
    pub data: Vec<u8>,
    pub value: Word,
    pub from: Address,
    pub to: Address,
    pub gas: Word,
}
