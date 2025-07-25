use serde::{Deserialize, Serialize};

use crate::common::{address::Address, word::Word};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Call {
    pub data: Vec<u8>,
    pub value: Word,
    pub from: Address,
    pub to: Address,
    pub gas: Word,
}
