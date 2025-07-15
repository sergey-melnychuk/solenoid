use primitive_types::U256;

use crate::common::address::Address;

#[derive(Clone, Debug)]
pub struct Call {
    pub calldata: Vec<u8>,
    pub value: U256,
    pub origin: Address,
    pub from: Address,
    pub to: Address,
    pub gas: U256,
}
