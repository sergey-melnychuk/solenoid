use std::collections::{HashMap, HashSet};

#[cfg(all(feature = "tracing", not(target_arch = "wasm32")))]
use std::time::Instant;

use crate::{
    common::{address::Address, block::AccessListItem, hash::keccak256, word::Word},
    eth::EthClient,
};

#[derive(Debug, Default)]
pub struct Account {
    pub value: Word,
    pub nonce: Word,
    pub root: Word,
    pub code: (Vec<u8>, Word),
    pub state: HashMap<Word, Word>,
}

struct Remote {
    eth: EthClient,
    block_hash: String,
}

#[derive(Default)]
pub struct Ext {
    remote: Option<Remote>,
    pub state: HashMap<Address, Account>,
    pub original: HashMap<(Address, Word), Word>,
    pub transient: HashMap<(Address, Word), Word>,

    // EIP-2929: Per-transaction access tracking
    pub accessed_addresses: HashSet<Address>,
    pub accessed_storage: HashSet<(Address, Word)>,
    pub created_accounts: Vec<Address>,
    pub destroyed_accounts: Vec<Address>,

    pub tx_ctx: TxContext,
}

#[derive(Clone, Default)]
pub struct TxContext {
    pub gas_price: Word,
    pub gas_max_fee: Word,
    pub blob_max_fee: Word,
    pub gas_max_priority_fee: Word,
    pub blob_gas_used: u64,
    pub access_list: Vec<AccessListItem>,
}

impl TxContext {
    pub fn access_list_cost(&self) -> i64 {
        let mut cost = 0i64;
        for item in &self.access_list {
            cost += 2400;
            cost += 1900 * item.storage_keys.len() as i64;
        }
        cost
    }
}

impl Ext {
    pub fn local() -> Self {
        Self::default()
    }

    pub fn at_hash(block_hash: String, eth: EthClient) -> Self {
        Self {
            remote: Some(Remote { eth, block_hash }),
            ..Default::default()
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

    pub fn apply_access_list(&mut self) {
        for item in self.tx_ctx.access_list.clone() {
            self.warm_address(&item.address);
            for key in &item.storage_keys {
                self.warm_storage(&item.address, key);
            }
        }
    }

    pub fn reset(&mut self, tx_ctx: TxContext) {
        self.tx_ctx = tx_ctx;
        self.original.clear();

        // Clear transient storage (EIP-1153)
        self.transient.clear();

        // Clear EIP-2929 access tracking
        self.accessed_addresses.clear();
        self.accessed_storage.clear();
        self.created_accounts.clear();

        // Apply SELFDESTRUCT
        let destroyed = self.destroyed_accounts.drain(..).collect::<Vec<_>>();
        for addr in destroyed {
            self.state.remove(&addr);
        }
    }

    /// Check if an address has been accessed in the current transaction (EIP-2929)
    pub fn is_address_warm(&self, addr: &Address) -> bool {
        self.accessed_addresses.contains(addr)
    }

    /// Mark an address as accessed in the current transaction (EIP-2929)
    pub fn warm_address(&mut self, addr: &Address) {
        self.accessed_addresses.insert(*addr);
    }

    /// Check if a storage slot has been accessed in the current transaction (EIP-2929)
    pub fn is_storage_warm(&self, addr: &Address, key: &Word) -> bool {
        self.accessed_storage.contains(&(*addr, *key))
    }

    /// Mark a storage slot as accessed in the current transaction (EIP-2929)
    pub fn warm_storage(&mut self, addr: &Address, key: &Word) {
        self.accessed_storage.insert((*addr, *key));
    }

    pub async fn get(&mut self, addr: &Address, key: &Word) -> eyre::Result<Word> {
        if !self.state.contains_key(addr) {
            self.pull(addr).await?;
        }
        if let Some(val) = self.state.get(addr).and_then(|s| s.state.get(key)).copied() {
            #[cfg(feature = "tracing")]
            tracing::debug!("GET: {addr}[{key:#x}]={val:#064x} [cached]");

            self.original.entry((*addr, *key)).or_insert(val);
            Ok(val)
        } else if let Some(Remote { eth, block_hash }) = self.remote.as_ref() {
            #[cfg(all(feature = "tracing", not(target_arch = "wasm32")))]
            let now = Instant::now();

            let hex = format!("{key:#064x}");
            let address = format!("0x{}", hex::encode(addr.0));
            let val = eth.get_storage_at(block_hash, &address, &hex).await?;

            #[cfg(all(feature = "tracing", not(target_arch = "wasm32")))]
            let ms = now.elapsed().as_millis();

            self.state.entry(*addr).or_default().state.insert(*key, val);
            self.original.entry((*addr, *key)).or_insert(val);

            #[cfg(all(feature = "tracing", not(target_arch = "wasm32")))]
            tracing::debug!("GET: {addr:#}[{key:#x}]={val:#064x} [remote {ms} ms]");

            Ok(val)
        } else {
            Ok(Word::zero())
        }
    }

    pub async fn put(&mut self, addr: &Address, key: Word, val: Word) -> eyre::Result<()> {
        self.state.entry(*addr).or_default().state.insert(key, val);

        #[cfg(all(feature = "tracing", not(target_arch = "wasm32")))]
        {
            let old = self.get(addr, &key).await?;
            let label = if old != val { "" } else { " [NOOP]" };
            tracing::debug!("PUT: {addr:#}[{key:#x}]={val:#x}{label}");
        }

        Ok(())
    }

    pub async fn is_empty(&mut self, addr: &Address) -> eyre::Result<bool> {
        let balance = self.balance(addr).await?;
        let nonce = self.nonce(addr).await?;
        let code = self.code(addr).await?;
        let is_empty = balance.is_zero() && nonce.is_zero() && code.0.is_empty();
        Ok(is_empty)
    }

    pub async fn code(&mut self, addr: &Address) -> eyre::Result<(Vec<u8>, Word)> {
        if let Some(code) = self.state.get(addr).map(|s| s.code.clone()) {
            Ok(code)
        } else {
            Ok(self.pull(addr).await?.code.clone())
        }
    }

    pub async fn balance(&mut self, addr: &Address) -> eyre::Result<Word> {
        if let Some(value) = self.state.get(addr).map(|s| s.value) {
            Ok(value)
        } else {
            Ok(self.pull(addr).await?.value)
        }
    }

    pub async fn nonce(&mut self, addr: &Address) -> eyre::Result<Word> {
        if let Some(nonce) = self.state.get(addr).map(|s| s.nonce) {
            // EIP-161 nonce=1 is now set explicitly in executor.rs when CREATE succeeds
            Ok(nonce)
        } else {
            Ok(self.pull(addr).await?.nonce)
        }
    }

    pub async fn get_block_hash(&mut self, block_number: Word) -> eyre::Result<Word> {
        if let Some(Remote { eth, .. }) = self.remote.as_ref() {
            let header = eth.get_block_header(block_number).await?;
            Ok(header.hash)
        } else {
            Ok(Word::zero())
        }
    }

    pub async fn pull(&mut self, addr: &Address) -> eyre::Result<&Account> {
        if self.state.contains_key(addr) {
            return Ok(self.state.get(addr).expect("must be present"));
        }
        if let Some(Remote { eth, block_hash }) = self.remote.as_ref() {
            let address = format!("0x{}", hex::encode(addr.0));
            let value = eth.get_balance(block_hash, &address).await?;
            let nonce = eth.get_nonce(block_hash, &address).await?;
            let code = eth.get_code(block_hash, &address).await?;
            let hash = Word::from_bytes(&keccak256(&code));
            let account = Account {
                value,
                nonce,
                code: (code, hash),
                root: Word::zero(),
                state: Default::default(),
            };
            self.state.insert(*addr, account);
            Ok(self.state.get(addr).expect("must always be present"))
        } else {
            eyre::bail!("failed to pull account {addr}")
        }
    }

    pub fn account_mut(&mut self, addr: &Address) -> &mut Account {
        if let Some(account) = self.state.get_mut(addr) {
            return account;
        }
        panic!("missing account {addr}")
    }

    pub fn state_mut(&mut self, addr: &Address) -> &mut HashMap<Word, Word> {
        &mut self.account_mut(addr).state
    }

    pub fn code_mut(&mut self, addr: &Address) -> &mut (Vec<u8>, Word) {
        &mut self.account_mut(addr).code
    }
}
