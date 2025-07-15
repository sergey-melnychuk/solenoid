use std::{collections::HashMap, time::Instant};

use primitive_types::U256;

use crate::{
    common::{account::Account, address::Address},
    eth::EthClient,
};

#[derive(Default)]
pub struct State {
    account: Account,
    data: HashMap<U256, U256>,
    code: Vec<u8>,
}

pub struct Ext {
    block_hash: String,
    state: HashMap<Address, State>,
    eth: EthClient,
}

impl Ext {
    pub fn new(block_hash: String, eth: EthClient) -> Self {
        Self {
            block_hash,
            state: Default::default(),
            eth,
        }
    }

    pub async fn get(&mut self, addr: &Address, key: &U256) -> eyre::Result<U256> {
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
            let ms = now.elapsed().as_millis();
            let addr = hex::encode(addr.0);
            tracing::info!("SLOAD: [{ms} ms] 0x{addr}[{key:#x}]={val:#x}");
            val
        };
        Ok(val)
    }

    pub async fn put(&mut self, addr: &Address, key: U256, val: U256) -> eyre::Result<()> {
        let state = self.state.entry(*addr).or_default();
        state.data.insert(key, val);
        Ok(())
    }

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

    pub fn acc_mut(&mut self, addr: &Address) -> Option<&mut Account> {
        self.state.get_mut(addr).map(|s| &mut s.account)
    }

    pub fn code_mut(&mut self, addr: &Address) -> Option<&mut Vec<u8>> {
        self.state.get_mut(addr).map(|s| &mut s.code)
    }
}
