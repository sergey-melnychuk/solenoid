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
    #[serde(flatten)]
    pub gas_info: TxGas,
    #[serde(rename = "blobVersionedHashes", default)]
    pub blob_versioned_hashes: Option<Vec<Word>>,
    #[serde(rename = "accessList", default)]
    pub access_list: Vec<AccessListItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccessListItem {
    pub address: Address,
    #[serde(rename = "storageKeys", default)]
    pub storage_keys: Vec<Word>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxGas {
    #[serde(rename = "gasPrice", default)]
    pub price: Option<Word>,
    #[serde(rename = "maxFeePerGas", default)]
    pub max_fee: Option<Word>,
    #[serde(rename = "maxPriorityFeePerGas", default)]
    pub max_priority_fee: Option<Word>,
    #[serde(rename = "maxFeePerBlobGas", default)]
    pub max_fee_per_blob: Option<Word>,
}

impl Tx {
    /// Calculate the effective gas price for this transaction
    /// For EIP-1559: min(maxFeePerGas, baseFeePerGas + maxPriorityFeePerGas)
    /// For legacy: gasPrice
    pub fn effective_gas_price(&self, base_fee: Word) -> Word {
        if let (Some(max_fee), Some(max_priority)) =
            (self.gas_info.max_fee, self.gas_info.max_priority_fee)
        {
            // EIP-1559 transaction
            let base_plus_priority = base_fee + max_priority;
            Word::min(max_fee, base_plus_priority)
        } else {
            // Legacy transaction - use gasPrice (default to 0 if not present)
            self.gas_info.price.unwrap_or_default()
        }
    }

    /// Check if this is a blob transaction (EIP-4844)
    pub fn is_blob_transaction(&self) -> bool {
        self.blob_versioned_hashes
            .as_ref()
            .is_some_and(|h| !h.is_empty())
    }

    /// Calculate the number of blobs in this transaction
    pub fn blob_count(&self) -> usize {
        self.blob_versioned_hashes.as_ref().map_or(0, |h| h.len())
    }
}

impl Header {
    /// Calculate blob gas price from excess_blob_gas per EIP-4844
    /// blob_gas_price = fake_exponential(excess_blob_gas, BLOB_BASE_FEE_UPDATE_FRACTION)
    pub fn blob_gas_price(&self) -> Word {
        if self.excess_blob_gas.is_zero() {
            return Word::from(1);
        }
        // Simplified calculation: blob_gas_price = MIN_BLOB_GASPRICE * e^(excess_blob_gas / BLOB_GASPRICE_UPDATE_FRACTION)
        // For approximation, we use the fake_exponential function from EIP-4844
        fake_exponential(
            self.excess_blob_gas.as_u64() as u128,
            3338477, // BLOB_GASPRICE_UPDATE_FRACTION
        )
    }
}

/// EIP-4844 fake exponential function
/// Returns e^(factor / denominator) approximation
fn fake_exponential(factor: u128, denominator: u128) -> Word {
    let mut output: u128 = 0;
    let mut numerator: u128 = denominator;
    let mut i = 1u128;

    while numerator > 0 {
        output += numerator;
        numerator = numerator.saturating_mul(factor) / (denominator.saturating_mul(i));
        i += 1;
    }
    Word::from(output / denominator)
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
    #[serde(rename = "blobGasUsed", default)]
    pub blob_gas_used: Word,
    #[serde(rename = "excessBlobGas", default)]
    pub excess_blob_gas: Word,
    #[serde(rename = "extraData")]
    pub extra_data: Word,
    pub miner: Address,
}
