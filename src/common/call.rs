use crate::common::{Word, address::Address};

#[derive(Clone, Debug)]
pub struct Call {
    pub calldata: Vec<u8>,
    pub value: Word,
    pub origin: Address,
    pub from: Address,
    pub to: Address,
    pub gas: Word,
}
