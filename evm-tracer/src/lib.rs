use alloy_consensus::Transaction as _;
use alloy_eips::BlockId;
use alloy_provider::Provider;
use alloy_rpc_types::{Header, Transaction as Tx};
use anyhow::Result;
use revm::context::result::{ExecResultAndState, ExecutionResult};
use revm::context::{ContextTr, JournalTr};
use revm::inspector::Inspector;
use revm::interpreter::{
    interpreter_types::{Jumps, LoopControl, MemoryTr, StackTr},
    CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter,
    InterpreterTypes,
};
use revm::{
    context::{Context, TxEnv},
    database::{AlloyDB, CacheDB, StateBuilder, WrapDatabaseAsync},
    primitives::{Address, Bytes, Log, TxKind, B256, U256},
    MainBuilder,
};
use revm::{InspectEvm, MainContext};
use serde::{Deserialize, Serialize};

pub use alloy_rpc_types;
pub use alloy_provider;
pub use alloy_consensus;
pub use alloy_eips;
pub use anyhow;

pub async fn trace(
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

    let mut tracer = TxTrace::new(
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpcodeTrace {
    pub pc: u64,
    pub opcode: u8,
    pub gas_remaining: u64,
    pub gas_cost: u64,
    pub stack: u64,  //Vec<U256>, // TODO: capture full stack?
    pub memory: u64, // TODO: capture full memory?
    pub depth: usize,
    pub refunded: i64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    gas: u64,
    #[serde(skip)]
    refunded: i64,
}

impl TxTrace {
    pub fn new(
        hash: B256,
        from: Address,
        to: Address,
        value: U256,
        gas_limit: u64,
    ) -> Self {
        Self {
            hash,
            from,
            to,
            value,
            gas_limit,
            gas_used: 0,
            success: false,
            return_data: Bytes::new(),
            traces: Vec::new(),
            gas: 0,
            refunded: 0,
        }
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
        self.gas = interp.gas.remaining();
        self.refunded = interp.gas.refunded();
    }

    fn step_end(&mut self, interp: &mut Interpreter<INTR>, context: &mut CTX) {
        let pc = interp.bytecode.pc();
        let opcode = interp.bytecode.opcode();

        let stack = interp.stack.len() as u64;
        let memory = interp.memory.size() as u64;

        let error = interp
            .bytecode
            .action()
            .as_ref()
            .and_then(|a| a.instruction_result())
            .map(|ir| format!("{ir:?}"));

        let gas_remaining = interp.gas.remaining();
        let gas_cost = self.gas - gas_remaining;
        self.gas = gas_remaining;

        let refunded = interp.gas.refunded() - self.refunded;
        self.refunded = interp.gas.refunded();

        self.traces.push(OpcodeTrace {
            pc: pc as u64,
            opcode,
            gas_remaining,
            gas_cost,
            stack,
            memory,
            depth: context.journal_mut().depth(),
            refunded,
            error,
        });
    }

    fn call(
        &mut self,
        _context: &mut CTX,
        _inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        //
        None
    }

    fn call_end(
        &mut self,
        context: &mut CTX,
        _inputs: &CallInputs,
        outcome: &mut CallOutcome,
    ) {
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
