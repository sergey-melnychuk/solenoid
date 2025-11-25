use alloy_consensus::Transaction as _;
use alloy_eips::BlockId;
use alloy_primitives::{TxKind, U256};
use alloy_provider::Provider;
use alloy_rpc_types::{Header, Transaction as Tx};
use eyre::Result;
use revm::context::result::{ExecResultAndState, ExecutionResult};
use revm::{
    context::{Context, TxEnv},
    database::{AlloyDB, CacheDB, StateBuilder, WrapDatabaseAsync},
    MainBuilder,
};
use revm::{ExecuteCommitEvm as _, InspectEvm, MainContext};

use crate::{OpcodeTrace, TxTrace};

pub struct TxResult {
    pub gas: i64,
    pub ret: Vec<u8>,
    pub rev: bool,
}

impl From<ExecResultAndState<ExecutionResult>> for TxResult {
    fn from(value: ExecResultAndState<ExecutionResult>) -> Self {
        Self {
            gas: value.result.gas_used() as i64,
            ret: value.result.output().map(|bytes| bytes.to_vec()).unwrap_or_default(),
            rev: !value.result.is_success(),
        }
    }
}

pub fn runner(
    header: Header,
    client: impl Provider + 'static,
) -> impl FnMut(Tx) -> Result<(TxResult, Vec<OpcodeTrace>)>
{
    let prev_id: BlockId = (header.number - 1).into();
    let state_db =
        WrapDatabaseAsync::new(AlloyDB::new(client, prev_id))
            .expect("can only fail if tokio runtime is unavailable");
    let cache_db = CacheDB::new(state_db);
    let state = StateBuilder::new_with_database(cache_db).build();

    let ctx = Context::mainnet()
        .with_db(state)
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
            c.disable_nonce_check = true;
        });

    let tracer = TxTrace::new();
    let mut evm = ctx.build_mainnet_with_inspector(tracer);

    move |tx: Tx| {
        let tx_env = TxEnv::builder()
            .caller(tx.inner.signer())
            .gas_limit(tx.gas_limit())
            .value(tx.value())
            .data(tx.input().to_owned())
            .chain_id(Some(1))
            .nonce(tx.nonce())
            .gas_price(tx.gas_price().unwrap_or(tx.inner.max_fee_per_gas()))
            .gas_priority_fee(tx.max_priority_fee_per_gas())
            // .access_list(tx.access_list().cloned().unwrap_or_default())
            .kind(match tx.to() {
                Some(to_address) => TxKind::Call(to_address),
                None => TxKind::Create,
            })
            .build()
            .expect("tx env");

        evm.inspector.setup(tx.info().hash.unwrap_or_default());

        let result = evm.inspect_tx(tx_env)?;
        // Note: Always commit state changes, since gas fees must be charged even for reverted transactions.
        // In Ethereum, miners get paid for computational work regardless of transaction success/failure.
        evm.commit(result.state.clone());

        // use revm::{context::ContextTr, handler::EvmTr};
        // let coinbase_balance = evm.ctx().db_ref().cache.accounts.get(&header.beneficiary)
        //     .and_then(|acc| acc.account_info())
        //     .unwrap_or_default()
        //     .balance;
        // println!("\n[REVM] COINBASE BALANCE: {coinbase_balance:#x}");

        let (_, traces) = evm.inspector.reset();
        Ok((result.into(), traces))
    }
}

