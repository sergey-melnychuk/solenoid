use std::{collections::HashMap, time::Instant};

use crate::{
    common::{
        account::Account,
        address::Address,
        hash::{self, keccak256},
        word::Word,
    },
    eth::EthClient,
};

#[derive(Default)]
pub struct State {
    pub account: Account,
    pub data: HashMap<Word, Word>,
    pub code: (Vec<u8>, Word),
}

struct Remote {
    eth: EthClient,
    block_hash: String,
}

#[derive(Default)]
pub struct Ext {
    remote: Option<Remote>,
    pub state: HashMap<Address, State>,
    pub original: HashMap<(Address, Word), Word>,
    pub transient: HashMap<Word, Word>,
}

impl Ext {
    pub fn local() -> Self {
        Self::default()
    }

    pub fn at_hash(block_hash: String, eth: EthClient) -> Self {
        Self {
            remote: Some(Remote { eth, block_hash }),
            state: Default::default(),
            original: HashMap::default(),
            transient: HashMap::default(),
        }
    }

    pub async fn at_number(number: Word, eth: EthClient) -> eyre::Result<Self> {
        let (_, block_hash) = eth.get_block_by_number(number).await?;
        Ok(Self::at_hash(block_hash, eth))
    }

    pub async fn at_latest(eth: EthClient) -> eyre::Result<Self> {
        let (_, block_hash) = eth.get_latest_block().await?;
        Ok(Self::at_hash(block_hash, eth))
    }

    pub async fn get(&mut self, addr: &Address, key: &Word) -> eyre::Result<Word> {
        if let Some(val) = self.state.get(addr).and_then(|s| s.data.get(key)).copied() {
            Ok(val)
        } else if let Some(Remote { eth, block_hash }) = self.remote.as_ref() {
            let now = Instant::now();
            let hex = format!("0x{key:064x}");
            let address = format!("0x{}", hex::encode(addr.0));
            let val = eth.get_storage_at(block_hash, &address, &hex).await?;
            let ms = now.elapsed().as_millis();

            self.state.entry(*addr).or_default().data.insert(*key, val);
            self.original.entry((*addr, *key)).or_insert(val);

            let addr = hex::encode(addr.0);
            tracing::info!("SLOAD  (rpc): 0x{addr}[{key:#x}]={val:#x} [took {ms} ms]");
            Ok(val)
        } else {
            Ok(Word::zero())
        }
    }

    pub async fn put(&mut self, addr: &Address, key: Word, val: Word) -> eyre::Result<()> {
        let state = self.state.entry(*addr).or_default();
        state.data.insert(key, val);
        tracing::info!("SSTORE (mem): {addr}[{key:#x}]={val:#x}");
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

    pub async fn code(&mut self, addr: &Address) -> eyre::Result<(Vec<u8>, Word)> {
        if let Some(code) = self.state.get(addr).map(|s| s.code.clone()) {
            Ok(code)
        } else if let Some(Remote { eth, block_hash }) = self.remote.as_ref() {
            let address = format!("0x{}", hex::encode(addr.0));
            let code = eth.get_code(block_hash, &address).await?;
            let state = self.state.entry(*addr).or_default();
            let hash = Word::from_bytes(&keccak256(&code));
            state.code = (code.clone(), hash);
            Ok((code, hash))
        } else {
            Ok((vec![], Word::from_bytes(&hash::empty())))
        }
    }

    pub async fn balance(&mut self, addr: &Address) -> eyre::Result<Word> {
        if let Some(acc) = self.state.get(addr).map(|s| s.account.clone()) {
            Ok(acc.value)
        } else if let Some(Remote { eth, block_hash }) = self.remote.as_ref() {
            let address = format!("0x{}", hex::encode(addr.0));
            let balance = eth.get_balance(block_hash, &address).await?;
            let state = self.state.entry(*addr).or_default();
            state.account.value = balance;
            Ok(balance)
        } else {
            Ok(Word::zero())
        }
    }

    pub async fn nonce(&mut self, addr: &Address) -> eyre::Result<Word> {
        if let Some(acc) = self.state.get(addr).map(|s| s.account.clone()) {
            Ok(acc.nonce)
        } else if let Some(Remote { eth, block_hash }) = self.remote.as_ref() {
            let address = format!("0x{}", hex::encode(addr.0));
            let nonce = eth.get_nonce(block_hash, &address).await?;
            let state = self.state.entry(*addr).or_default();
            state.account.nonce = nonce;
            Ok(nonce)
        } else {
            Ok(Word::zero())
        }
    }

    pub async fn pull(&mut self, addr: &Address) -> eyre::Result<Account> {
        let (_code, _hash) = self.code(addr).await?;
        let balance = self.balance(addr).await?;
        let nonce = self.nonce(addr).await?;
        Ok(Account {
            value: balance,
            nonce,
            root: Word::zero(),
        })
    }

    pub fn acc_mut(&mut self, addr: &Address) -> &mut Account {
        &mut self.state.entry(*addr).or_default().account
    }

    pub fn code_mut(&mut self, addr: &Address) -> &mut (Vec<u8>, Word) {
        &mut self.state.entry(*addr).or_default().code
    }

    pub fn data_mut(&mut self, addr: &Address) -> &mut HashMap<Word, Word> {
        &mut self.state.entry(*addr).or_default().data
    }
}
