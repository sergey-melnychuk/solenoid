use serde::{Deserialize, Serialize};

use crate::common::{Hex, address::Address, word::Word};

#[derive(Debug, Serialize, Deserialize)]
pub struct Tx {
    pub hash: Word,
    #[serde(rename = "transactionIndex")]
    pub index: Word,
    pub from: Address,
    pub gas: Word,
    pub input: Hex,
    pub to: Option<Address>,
    pub value: Word,
    #[serde(rename = "gasPrice")]
    pub gas_price: Word,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Block {
    #[serde(flatten)]
    pub header: Header,
    pub transactions: Vec<Tx>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Header {
    pub number: Word,
    pub hash: Word,
    pub size: Word,
    pub timestamp: Word,
    #[serde(rename = "baseFeePerGas")]
    pub base_fee: Word,
    #[serde(rename = "stateRoot")]
    pub state_root: Word,
    #[serde(rename = "mixHash")]
    pub mix_hash: Word,
    #[serde(rename = "parentHash")]
    pub parent_hash: Word,
    #[serde(rename = "gasLimit")]
    pub gas_limit: Word,
    #[serde(rename = "gasUsed")]
    pub gas_used: Word,
    #[serde(rename = "blobGasUsed")]
    pub blob_gas_used: Word,
    #[serde(rename = "extraData")]
    pub extra_data: Word,
}
