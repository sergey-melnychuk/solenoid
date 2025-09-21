use alloy_consensus::Transaction as _;
use alloy_eips::BlockId;
use alloy_provider::Provider;
use alloy_rpc_types::{Header, Transaction as Tx};
use eyre::Result;
use revm::context::result::{ExecResultAndState, ExecutionResult};
use revm::context::{ContextTr, JournalTr};
use revm::inspector::Inspector;
use revm::interpreter::{
    interpreter_types::{Jumps, MemoryTr, StackTr},
    CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter,
    InterpreterTypes,
};
use revm::{
    context::{Context, TxEnv},
    database::{AlloyDB, CacheDB, StateBuilder, WrapDatabaseAsync},
    primitives::{Address, Bytes, Log, TxKind, B256, U256},
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

pub async fn trace_all(
    txs: impl Iterator<Item = Tx>,
    header: &Header,
    client: impl Provider + Clone,
) -> Result<Vec<(ExecResultAndState<ExecutionResult>, TxTrace)>> {
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
            c.chain_id = 1;
        });

    let mut tracer = TxTrace::default();
    let mut evm = ctx.build_mainnet_with_inspector(&mut tracer);

    let mut ret = Vec::new();
    for tx in txs {
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

        evm.inspector.setup(
            tx.info().hash.unwrap_or_default(),
            tx.inner.signer(),
            tx.to().unwrap_or_default(),
            tx.value(),
            tx.gas_limit(),
        );

        let result = evm.inspect_tx(tx_env)?;
        if result.result.is_success() {
            evm.commit(result.state.clone());
        }

        let tracer = evm.inspector.reset();
        ret.push((result, tracer));
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
            c.chain_id = 1;
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
    tracer.setup(
        tx.info().hash.unwrap_or_default(),
        tx.inner.signer(),
        tx.to().unwrap_or_default(),
        tx.value(),
        tx.gas_limit(),
    );

    let mut evm = ctx.build_mainnet_with_inspector(&mut tracer);
    let result = evm.inspect_tx(tx_env)?;
    Ok((result, tracer))
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct OpcodeTrace {
    pub pc: u64,
    pub op: u8,
    pub name: String,
    pub gas_used: u64,
    // pub gas_left: u64, // NOTE: temporary disabled
    pub gas_cost: u64,
    pub gas_back: i64,
    pub stack: Vec<U256>,
    pub memory: Vec<U256>,
    pub depth: usize,

    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extra {
    #[serde(flatten)]
    pub value: serde_json::Value,
}

impl Extra {
    pub fn new(value: Value) -> Self {
        Self { value }
    }
}

impl PartialEq for Extra {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Extra {}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TxTrace {
    pub hash: B256,
    pub from: Address,
    pub to: Address,
    pub value: U256,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub success: bool,
    pub return_data: Bytes,
    pub traces: Vec<OpcodeTrace>,

    #[serde(skip)]
    aux: Aux,
}

#[derive(Clone, Debug, Default)]
pub struct Aux {
    pc: u64,
    opcode: u8,
    gas: u64,
    refund: i64,
    depth: usize,
}

impl TxTrace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn setup(
        &mut self,
        hash: B256,
        from: Address,
        to: Address,
        value: U256,
        gas_limit: u64,
    ) {
        *self = Self {
            hash,
            from,
            to,
            value,
            gas_limit,
            gas_used: 0,
            success: false,
            return_data: Bytes::new(),
            traces: Vec::new(),
            aux: Aux::default(),
        }
    }

    pub fn reset(&mut self) -> Self {
        let ret = self.clone();
        *self = Self::default();
        ret
    }
}

impl<CTX, INTR> Inspector<CTX, INTR> for TxTrace
where
    CTX: ContextTr,
    INTR: InterpreterTypes,
{
    fn initialize_interp(
        &mut self,
        _interp: &mut Interpreter<INTR>,
        _context: &mut CTX,
    ) {
        //
    }

    fn step(&mut self, interp: &mut Interpreter<INTR>, _context: &mut CTX) {
        self.aux.pc = interp.bytecode.pc() as u64;
        self.aux.opcode = interp.bytecode.opcode();
        self.aux.gas = interp.gas.remaining();
        self.aux.refund = interp.gas.refunded();
    }

    fn step_end(&mut self, interp: &mut Interpreter<INTR>, _context: &mut CTX) {
        let stack = interp.stack.data().to_vec();
        let memory = interp.memory.slice(0..interp.memory.size()).to_vec();

        let refund = interp.gas.refunded() - self.aux.refund;
        self.aux.refund = interp.gas.refunded();

        let gas_cost = self.aux.gas - interp.gas.remaining();
        self.aux.gas = interp.gas.remaining();

        self.traces.push(OpcodeTrace {
            pc: self.aux.pc,
            op: self.aux.opcode,
            name: aux::opcode_name(self.aux.opcode).to_string(),
            gas_used: interp.gas.spent(),
            // gas_left: interp.gas.remaining(),
            gas_cost,
            gas_back: refund,
            stack,
            memory: memory
                .chunks(32)
                .map(|chunk| U256::from_be_slice(chunk))
                .collect(),
            depth: self.aux.depth,
            extra: Extra::new(json!({
                "gas_left": interp.gas.remaining()
            })),
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
        context: &mut CTX,
        _inputs: &CallInputs,
        outcome: &mut CallOutcome,
    ) {
        self.aux.depth -= 1;
        if context.journal_mut().depth() == 0 {
            // This is the top-level call ending
            self.success = outcome.result.is_ok();
            self.return_data = outcome.result.output.clone();
            self.gas_used = self
                .gas_limit
                .saturating_sub(outcome.result.gas.remaining());
        }
    }

    fn create_end(
        &mut self,
        context: &mut CTX,
        _inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        self.aux.depth -= 1;
        if context.journal_mut().depth() == 0 {
            // This is the top-level create ending
            self.success = outcome.result.is_ok();
            self.return_data = outcome.result.output.clone();
            self.gas_used = self
                .gas_limit
                .saturating_sub(outcome.result.gas.remaining());
        }
    }

    fn log(
        &mut self,
        _interp: &mut Interpreter<INTR>,
        _context: &mut CTX,
        _log: Log,
    ) {
        // Could add log tracking here if needed
    }
}
