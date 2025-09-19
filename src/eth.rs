use eyre::OptionExt;

use crate::common::{
    block::{Block, Header},
    word::Word,
};

#[cfg(feature = "account")]
use crate::common::account::Account;

#[derive(Clone)]
pub struct EthClient {
    http: reqwest::Client,
    url: String,
}

impl EthClient {
    pub fn new(url: &str) -> Self {
        let http = reqwest::ClientBuilder::new()
            .build()
            .expect("Failed to create HTTP client");
        Self {
            http,
            url: url.to_string(),
        }
    }

    pub async fn get_block_header(&self, number: Word) -> eyre::Result<Header> {
        let value = self
            .rpc(serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": [
                    number,
                    false
                ],
                "id": 0
            }))
            .await?;
        let header = serde_json::from_value(value)?;
        Ok(header)
    }

    pub async fn get_full_block(&self, number: Word) -> eyre::Result<Block> {
        let value = self
            .rpc(serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": [
                    number,
                    true
                ],
                "id": 0
            }))
            .await?;
        let block = serde_json::from_value(value)?;
        Ok(block)
    }

    pub async fn get_block_by_number(&self, number: Word) -> eyre::Result<(u64, String)> {
        self.rpc(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [
                number,
                false
            ],
            "id": 0
        }))
        .await
        .and_then(|value| {
            let num = hex_to_u64(&value["number"])?;
            let hash = value["hash"]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| eyre::eyre!("block hash missing"))?;
            Ok((num, hash))
        })
    }

    pub async fn get_latest_block(&self) -> eyre::Result<(u64, String)> {
        self.rpc(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [
                "latest",
                false
            ],
            "id": 0
        }))
        .await
        .and_then(|value| {
            let num = hex_to_u64(&value["number"])?;
            let hash = value["hash"]
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| eyre::eyre!("block hash missing"))?;
            Ok((num, hash))
        })
    }

    pub async fn get_storage_at(
        &self,
        block_hash: &str,
        address: &str,
        key: &str,
    ) -> eyre::Result<Word> {
        self.rpc(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getStorageAt",
            "params": [
                address,
                key,
                {
                    "blockHash": block_hash,
                }
            ],
            "id": 0
        }))
        .await
        .and_then(|value| hex_to_word(&value))
    }

    pub async fn get_code(&self, block_hash: &str, address: &str) -> eyre::Result<Vec<u8>> {
        self.rpc(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getCode",
            "params": [
                address,
                {
                    "blockHash": block_hash,
                }
            ],
            "id": 0
        }))
        .await
        .and_then(|value| hex_to_vec(&value))
    }

    pub async fn get_balance(&self, block_hash: &str, address: &str) -> eyre::Result<Word> {
        self.rpc(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBalance",
            "params": [
                address,
                {
                    "blockHash": block_hash,
                }
            ],
            "id": 0
        }))
        .await
        .and_then(|value| hex_to_word(&value))
    }

    pub async fn get_nonce(&self, block_hash: &str, address: &str) -> eyre::Result<Word> {
        self.rpc(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionCount",
            "params": [
                address,
                {
                    "blockHash": block_hash,
                }
            ],
            "id": 0
        }))
        .await
        .and_then(|value| hex_to_word(&value))
    }

    #[cfg(feature = "account")]
    pub async fn get_account(&self, block_hash: &str, address: &str) -> eyre::Result<Account> {
        self.rpc(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getAccount",
            "params": [
                address,
                {
                    "blockHash": block_hash,
                }
            ],
            "id": 0
        }))
        .await
        .and_then(|value| parse_account(&value))
    }

    pub async fn call(
        &self,
        block_hash: &str,
        address: &str,
        calldata: &str,
    ) -> eyre::Result<Vec<u8>> {
        self.rpc(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [
                {
                    "to": address,
                    "data": calldata,
                },
                {
                    "blockHash": block_hash,
                }
            ],
            "id": 0
        }))
        .await
        .and_then(|value| hex_to_vec(&value))
    }

    async fn rpc(&self, value: serde_json::Value) -> eyre::Result<serde_json::Value> {
        let res = self.http.post(&self.url).json(&value).send().await?;

        let status = res.status();
        let (code, message) = (status.as_u16(), status.as_str());
        if !status.is_success() {
            tracing::error!(code, message, "Ethereum call failed");
            eyre::bail!(code);
        }

        let response: serde_json::Value = res.json().await?;

        if let Some(error) = response["error"].as_object() {
            let json = serde_json::to_string(&error)?;
            eyre::bail!("RPC error: '{json}'");
        }
        if let Some(error) = response["error"].as_str() {
            eyre::bail!("RPC error: '{error}'");
        }
        Ok(response["result"].clone())
    }
}

fn hex_to_word(val: &serde_json::Value) -> eyre::Result<Word> {
    let hex = val.as_str().ok_or_eyre("missing hex str")?;
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    let num = Word::from_hex(hex)?;
    Ok(num)
}

fn hex_to_u64(val: &serde_json::Value) -> eyre::Result<u64> {
    let hex = val.as_str().ok_or_eyre("missing hex str")?;
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    let num = u64::from_str_radix(hex, 16)?;
    Ok(num)
}

fn hex_to_vec(val: &serde_json::Value) -> eyre::Result<Vec<u8>> {
    let hex = val.as_str().ok_or_eyre("missing hex str")?;
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    let vec = hex::decode(hex)?;
    Ok(vec)
}

#[cfg(feature = "account")]
fn parse_account(val: &serde_json::Value) -> eyre::Result<Account> {
    Ok(Account {
        balance: val
            .get("balance")
            .ok_or_eyre("account.balance missing")
            .and_then(hex_to_word)?,
        nonce: val
            .get("nonce")
            .ok_or_eyre("account.balance missing")
            .and_then(hex_to_word)?,
        code_hash: val
            .get("codeHash")
            .ok_or_eyre("account.codeHash missing")
            .and_then(hex_to_word)?,
        root: val
            .get("storageRoot")
            .ok_or_eyre("account.storageRoot missing")
            .and_then(hex_to_word)?,
    })
}

#[cfg(test)]
mod tests {
    use crate::common::hash::keccak256;

    fn get_method_selector(signature: &str) -> String {
        let hash = keccak256(signature.as_bytes());
        format!("0x{}", hex::encode(&hash[0..4]))
    }

    #[test]
    fn test_selectors() {
        assert_eq!(
            get_method_selector("transfer(address,uint256)"),
            "0xa9059cbb"
        );
        assert_eq!(get_method_selector("get()"), "0x6d4ce63c");
        assert_eq!(get_method_selector("set(uint256)"), "0x60fe47b1");
    }

    #[test]
    fn test_empty() {
        assert_eq!(get_method_selector(""), "0xc5d24601");
    }
}
