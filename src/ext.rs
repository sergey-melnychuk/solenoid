use std::{collections::HashMap, time::Instant};

use crate::{
    common::{Word, account::Account, address::Address},
    eth::EthClient,
};

#[derive(Default)]
pub struct State {
    pub account: Account,
    pub data: HashMap<Word, Word>,
    pub code: Vec<u8>,
}

pub struct Ext {
    eth: EthClient,
    block_hash: String,
    pub state: HashMap<Address, State>,
    pub original: HashMap<(Address, Word), Word>,
}

impl Ext {
    pub fn new(block_hash: String, eth: EthClient) -> Self {
        Self {
            eth,
            block_hash,
            state: Default::default(),
            original: HashMap::default(),
        }
    }

    pub async fn latest(eth: EthClient) -> eyre::Result<Self> {
        let (_, block_hash) = eth.get_latest_block().await?;
        Ok(Self::new(block_hash, eth))
    }

    pub async fn get(&mut self, addr: &Address, key: &Word) -> eyre::Result<Word> {
        let val = if let Some(val) = self.state.get(addr).and_then(|s| s.data.get(key)).copied() {
            val
        } else {
            let now = Instant::now();
            let hex = format!("0x{key:064x}");
            let address = format!("0x{}", hex::encode(addr.0));
            let val = self
                .eth
                .get_storage_at(&self.block_hash, &address, &hex)
                .await?;
            self.original.entry((*addr, *key)).or_insert(val);
            let ms = now.elapsed().as_millis();
            let addr = hex::encode(addr.0);
            tracing::info!("SLOAD: [{ms} ms] 0x{addr}[{key:#x}]={val:#x}");
            val
        };
        Ok(val)
    }

    pub async fn put(&mut self, addr: &Address, key: Word, val: Word) -> eyre::Result<()> {
        let state = self.state.entry(*addr).or_default();
        state.data.insert(key, val);
        Ok(())
    }

    #[cfg(feature = "account")]
    pub async fn acc(&mut self, addr: &Address) -> eyre::Result<Account> {
        if let Some(acc) = self.state.get(addr).map(|s| s.account.clone()) {
            Ok(acc)
        } else {
            let address = format!("0x{}", hex::encode(addr.0));
            let account = self.eth.get_account(&self.block_hash, &address).await?;

            let state = self.state.entry(*addr).or_default();
            state.account = account.clone();
            Ok(account)
        }
    }

    pub async fn code(&mut self, addr: &Address) -> eyre::Result<Vec<u8>> {
        if let Some(code) = self.state.get(addr).map(|s| s.code.clone()) {
            Ok(code)
        } else {
            let address = format!("0x{}", hex::encode(addr.0));
            let code = self.eth.get_code(&self.block_hash, &address).await?;

            let state = self.state.entry(*addr).or_default();
            state.code = code.clone();
            Ok(code)
        }
    }

    pub async fn balance(&mut self, addr: &Address) -> eyre::Result<Word> {
        if let Some(acc) = self.state.get(addr).map(|s| s.account.clone()) {
            Ok(acc.balance)
        } else {
            let address = format!("0x{}", hex::encode(addr.0));
            let balance = self.eth.get_balance(&self.block_hash, &address).await?;

            let state = self.state.entry(*addr).or_default();
            state.account.balance = balance;
            Ok(balance)
        }
    }

    pub fn acc_mut(&mut self, addr: &Address) -> &mut Account {
        &mut self.state.entry(*addr).or_default().account
    }

    pub fn code_mut(&mut self, addr: &Address) -> &mut Vec<u8> {
        &mut self.state.entry(*addr).or_default().code
    }
}
