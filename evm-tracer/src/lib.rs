use alloy_consensus::Transaction as _;
use alloy_eips::BlockId;
use alloy_primitives::Address;
use alloy_provider::Provider;
use alloy_rpc_types::{Header, Transaction as Tx};
use evm_common::word::Word;
use evm_event::{Event, EventData, OpCode};
use eyre::Result;
use revm::context::result::{ExecResultAndState, ExecutionResult};
use revm::context::ContextTr;
use revm::inspector::{Inspector, JournalExt};
use revm::interpreter::interpreter_types::InputsTr;
use revm::interpreter::{
    interpreter_types::{Jumps, MemoryTr, StackTr},
    CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter,
    InterpreterTypes,
};
use revm::primitives::hardfork::SpecId;
use revm::primitives::{StorageKey, StorageValue};
use revm::{
    context::{Context, TxEnv},
    database::{AlloyDB, CacheDB, StateBuilder, WrapDatabaseAsync},
    primitives::{TxKind, B256, U256},
    MainBuilder,
};
use revm::{ExecuteCommitEvm as _, InspectEvm, MainContext};
use serde::{Deserialize, Serialize};

pub use alloy_consensus;
pub use alloy_eips;
pub use alloy_primitives;
pub use alloy_provider;
pub use alloy_rpc_types;
pub use eyre;
pub use revm;
use serde_json::{json, Value};

pub mod aux;
pub mod run;

pub async fn trace_all(
    txs: impl Iterator<Item = Tx>,
    header: &Header,
    client: impl Provider + Clone,
) -> Result<Vec<(ExecResultAndState<ExecutionResult>, Vec<Event>)>> {
    let prev_id: BlockId = (header.number - 1).into();
    let state_db =
        WrapDatabaseAsync::new(AlloyDB::new(client.clone(), prev_id))
            .expect("can only fail if tokio runtime is unavailable");
    let cache_db = CacheDB::new(state_db);
    let mut state = StateBuilder::new_with_database(cache_db).build();

    let ctx = Context::mainnet()
        .with_db(&mut state)
        .modify_block_chained(|b| {
            b.number = U256::from(header.number);
            b.beneficiary = header.beneficiary;
            b.timestamp = U256::from(header.timestamp);
            b.difficulty = header.difficulty;
            b.gas_limit = header.gas_limit;
            b.basefee = header.base_fee_per_gas.unwrap_or_default();
        })
        .modify_cfg_chained(|c| {
            c.spec = SpecId::OSAKA;
            c.chain_id = 1;
            c.disable_nonce_check = true;
            c.disable_balance_check = true;
        })
        .modify_journal_chained(|j| {
            j.set_spec_id(SpecId::OSAKA.into());
        });

    let mut tracer = TxTrace::default();
    let mut evm = ctx.build_mainnet_with_inspector(&mut tracer);

    let mut ret = Vec::new();
    for tx in txs {
        // eprintln!("TX hash={} index={}", tx.inner.hash(), tx.transaction_index.unwrap_or_default());
        // eprintln!("TX: {tx:#?}");
        // eprintln!("GAS LIMIT: {}", tx.gas_limit());
        let tx_env = TxEnv::builder()
            .caller(tx.inner.signer())
            .gas_limit(tx.gas_limit())
            .value(tx.value())
            .data(tx.input().to_owned())
            .chain_id(Some(1))
            .nonce(tx.nonce())
            .gas_price(tx.gas_price().unwrap_or(tx.inner.max_fee_per_gas()))
            .gas_priority_fee(tx.max_priority_fee_per_gas())
            .access_list(tx.access_list().cloned().unwrap_or_default())
            .kind(match tx.to() {
                Some(to_address) => TxKind::Call(to_address),
                None => TxKind::Create,
            })
            .build()
            .unwrap();

        evm.inspector.setup(tx.info().hash.unwrap_or_default());

        let result = evm.inspect_tx(tx_env)?;
        if result.result.is_success() {
            evm.commit(result.state.clone());
        }

        let (_, traces) = evm.inspector.reset();
        ret.push((result, traces));
    }
    Ok(ret)
}

pub async fn trace_one(
    tx: Tx,
    header: &Header,
    client: impl Provider + Clone,
) -> Result<(ExecResultAndState<ExecutionResult>, TxTrace)> {
    let prev_id: BlockId = (header.number - 1).into();
    let state_db =
        WrapDatabaseAsync::new(AlloyDB::new(client.clone(), prev_id))
            .expect("can only fail if tokio runtime is unavailable");
    let cache_db = CacheDB::new(state_db);
    let mut state = StateBuilder::new_with_database(cache_db).build();

    let ctx = Context::mainnet()
        .with_db(&mut state)
        .modify_block_chained(|b| {
            b.number = U256::from(header.number);
            b.beneficiary = header.beneficiary;
            b.timestamp = U256::from(header.timestamp);
            b.difficulty = header.difficulty;
            b.gas_limit = header.gas_limit;
            b.basefee = header.base_fee_per_gas.unwrap_or_default();
        })
        .modify_cfg_chained(|c| {
            c.spec = SpecId::OSAKA;
            c.chain_id = 1;
            c.disable_nonce_check = true;
            c.disable_balance_check = true;
        })
        .modify_journal_chained(|j| {
            j.set_spec_id(SpecId::OSAKA.into());
        });

    let tx_env = TxEnv::builder()
        .caller(tx.inner.signer())
        .gas_limit(tx.gas_limit())
        .value(tx.value())
        .data(tx.input().to_owned())
        .chain_id(Some(1))
        .nonce(tx.nonce())
        .gas_price(tx.gas_price().unwrap_or(tx.inner.max_fee_per_gas()))
        .gas_priority_fee(tx.max_priority_fee_per_gas())
        .access_list(tx.access_list().cloned().unwrap_or_default())
        .kind(match tx.to() {
            Some(to_address) => TxKind::Call(to_address),
            None => TxKind::Create,
        })
        .build()
        .unwrap();

    let mut tracer = TxTrace::default();
    tracer.setup(tx.info().hash.unwrap_or_default());

    let mut evm = ctx.build_mainnet_with_inspector(&mut tracer);
    let result = evm.inspect_tx(tx_env)?;
    Ok((result, tracer))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugInfo {
    #[serde(flatten)]
    pub value: serde_json::Value,
}

impl DebugInfo {
    pub fn new(value: Value) -> Self {
        Self { value }
    }
}

impl PartialEq for DebugInfo {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for DebugInfo {}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TxTrace {
    pub hash: B256,
    pub traces: Vec<Event>,

    #[serde(skip)]
    aux: Aux,
    #[serde(skip)]
    sstore: Sstore,
}

#[derive(Clone, Debug, Default)]
pub struct Aux {
    pc: u64,
    opcode: u8,
    gas: i64,
    refund: i64,
    depth: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Sstore {
    key: StorageKey,
    val: StorageValue,
}

impl TxTrace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn setup(&mut self, hash: B256) {
        *self = Self {
            hash,
            traces: Vec::new(),
            aux: Aux::default(),
            sstore: Sstore::default(),
        }
    }

    pub fn reset(&mut self) -> (B256, Vec<Event>) {
        let traces = std::mem::take(&mut self.traces);
        let hash = self.hash;
        (hash, traces)
    }
}

impl<CTX, INTR> Inspector<CTX, INTR> for TxTrace
where
    CTX: ContextTr<Journal: JournalExt>,
    INTR: InterpreterTypes,
{
    fn step(&mut self, interp: &mut Interpreter<INTR>, _context: &mut CTX) {
        self.aux.pc = interp.bytecode.pc() as u64;
        self.aux.opcode = interp.bytecode.opcode();
        self.aux.gas = interp.gas.remaining() as i64;
        self.aux.refund = interp.gas.refunded();

        let stack = interp.stack.data();
        if self.aux.opcode == 0x55 && stack.len() >= 2 {
            // SSTORE: stack has [value, key] (top of stack is value, second is key)
            let key = stack[stack.len() - 1];
            let val = stack[stack.len() - 2];
            self.sstore.key = key.into();
            self.sstore.val = val.into();
        }
    }

    fn step_end(&mut self, interp: &mut Interpreter<INTR>, context: &mut CTX) {
        let stack = interp.stack.data().to_vec();
        let memory = interp.memory.slice(0..interp.memory.size()).to_vec();

        let refund = interp.gas.refunded() - self.aux.refund;
        self.aux.refund = interp.gas.refunded();

        let gas_cost = self.aux.gas - interp.gas.remaining() as i64;
        self.aux.gas = interp.gas.remaining() as i64;

        // Check storage warm/cold status for SSTORE (0x55)
        let mut debug_value = json!({
            "gas_left": interp.gas.remaining(),
            "evm.gas.back": interp.gas.refunded()
        });

        // Check storage warm/cold status for SSTORE (0x55) if JournalExt is available
        if self.aux.opcode == 0x55 {
            // Get the target address from the interpreter
            let target_address =
                <INTR::Input as InputsTr>::target_address(&interp.input);
            let journal = context.journal();
            use revm::inspector::JournalExt as _;
            let evm_state = journal.evm_state();
            if let Some(account) = evm_state.get(&target_address) {
                if let Some(slot) = account.storage.get(&self.sstore.key) {
                    let is_cold =
                        slot.is_cold_transaction_id(account.transaction_id);
                    debug_value["SSTORE"] = json!({
                        "is_warm": !is_cold,
                        "original": format!("0x{:x}", slot.original_value),
                        "address": format!("{:?}", target_address),
                        "key": format!("0x{:x}", self.sstore.key),
                        "val": format!("0x{:x}", slot.present_value),
                        "new": format!("0x{:x}", self.sstore.val),
                    });
                }
            }
        }

        self.traces.push(Event {
            data: EventData::OpCode(OpCode {
                pc: self.aux.pc as usize,
                op: self.aux.opcode,
                name: aux::opcode_name(self.aux.opcode).to_string(),
                data: None,
                gas_used: interp.gas.spent() as i64,
                gas_left: interp.gas.remaining() as i64,
                gas_cost,
                gas_back: refund,
                stack: stack
                    .into_iter()
                    .rev()
                    .map(|word| Word::from_bytes(&word.to_be_bytes::<32>()))
                    .collect(),
                memory: memory
                    .chunks(32)
                    .map(|chunk| Word::from_bytes(&chunk))
                    .collect(),
                debug: debug_value,
            }),
            depth: self.aux.depth,
            reverted: false,
        });
    }

    fn call(
        &mut self,
        _context: &mut CTX,
        _inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        self.aux.depth += 1;
        None
    }

    fn call_end(
        &mut self,
        _context: &mut CTX,
        _inputs: &CallInputs,
        _outcome: &mut CallOutcome,
    ) {
        if self.aux.depth > 0 {
            self.aux.depth -= 1;
        }
    }

    fn create(
        &mut self,
        _context: &mut CTX,
        _inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        self.aux.depth += 1;
        None
    }

    fn create_end(
        &mut self,
        _context: &mut CTX,
        _inputs: &CreateInputs,
        _outcome: &mut CreateOutcome,
    ) {
        if self.aux.depth > 0 {
            self.aux.depth -= 1;
        }
    }

    fn selfdestruct(
        &mut self,
        _contract: Address,
        _target: Address,
        _value: U256,
    ) {
        // ignore
    }
}
