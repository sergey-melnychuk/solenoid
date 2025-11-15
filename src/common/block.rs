use serde::{Deserialize, Serialize};

use crate::common::{Hex, address::Address, word::Word};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tx {
    pub hash: Word,
    #[serde(rename = "transactionIndex")]
    pub index: Word,
    pub from: Address,
    pub gas: Word,
    pub input: Hex,
    pub to: Option<Address>,
    pub value: Word,
    #[serde(rename = "gasPrice", default)]
    pub gas_price: Option<Word>,
    #[serde(rename = "maxFeePerGas", default)]
    pub max_fee_per_gas: Option<Word>,
    #[serde(rename = "maxPriorityFeePerGas", default)]
    pub max_priority_fee_per_gas: Option<Word>,
}

impl Tx {
    /// Calculate the effective gas price for this transaction
    /// For EIP-1559: min(maxFeePerGas, baseFeePerGas + maxPriorityFeePerGas)
    /// For legacy: gasPrice
    pub fn effective_gas_price(&self, base_fee: Word) -> Word {
        if let (Some(max_fee), Some(max_priority)) = (self.max_fee_per_gas, self.max_priority_fee_per_gas) {
            // EIP-1559 transaction
            let base_plus_priority = base_fee + max_priority;
            Word::min(max_fee, base_plus_priority)
        } else {
            // Legacy transaction - use gasPrice (default to 0 if not present)
            self.gas_price.unwrap_or_default()
        }
    }
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
    pub miner: Address,
}
