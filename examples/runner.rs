use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::{future::Future, pin::Pin, sync::Arc};

use evm_common::address::Address;
use evm_event::{Event, EventData};
use evm_tracer::alloy_eips::BlockNumberOrTag;
use evm_tracer::alloy_provider::{Provider, ProviderBuilder};
use evm_tracer::alloy_rpc_types::BlockTransactions;
use serde_json::Value;
use tokio::sync::Mutex;

use evm_common::block::{Block, Header, Tx};
use evm_common::word::Word;
use solenoid::{
    eth,
    ext::Ext,
    ext::TxContext,
    solenoid::{Builder as _, CallResult, Solenoid},
    tracer::{EventTracer as _, LoggingTracer},
};

use evm_tracer::run::TxResult;

async fn as_tx_result(
    gas_costs: i64,
    gas_floor: i64,
    result: &CallResult<LoggingTracer>,
    ext: &mut Ext,
) -> eyre::Result<TxResult> {
    let gas = result
        .evm
        .gas
        .finalized(gas_costs, result.evm.reverted)
        .max(gas_floor);
    let state = as_state(result, ext).await?;
    Ok(TxResult {
        gas,
        ret: result.ret.clone(),
        rev: result.evm.reverted,
        state,
    })
}

async fn as_state(
    result: &CallResult<LoggingTracer>,
    ext: &mut Ext,
) -> eyre::Result<BTreeMap<String, Value>> {
    let mut kv: BTreeMap<Address, BTreeSet<Word>> = BTreeMap::new();
    let touched = result
        .evm
        .touches
        .iter()
        .filter_map(|touch| match touch {
            solenoid::executor::AccountTouch::GetState(address, key, _, _) => {
                kv.entry(*address).or_default().insert(*key);
                Some(*address)
            }
            solenoid::executor::AccountTouch::GetCode(address, _, _) => Some(*address),
            // touches that modify the state:
            solenoid::executor::AccountTouch::FeePay(address, _, _) => Some(*address),
            solenoid::executor::AccountTouch::SetState(address, key, _, _, _) => {
                kv.entry(*address).or_default().insert(*key);
                Some(*address)
            }
            solenoid::executor::AccountTouch::SetNonce(address, _, _) => Some(*address),
            solenoid::executor::AccountTouch::SetValue(address, _, _) => Some(*address),
            solenoid::executor::AccountTouch::Create(address, _, _, _, _) => Some(*address),
            _ => None,
        })
        .collect::<BTreeSet<_>>();

    let mut ret: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for address in touched.into_iter() {
        let account = ext.pull(&address).await?;
        let mut json = serde_json::json!({
            "balance": account.value,
            "nonce": account.nonce.as_u64(),
            "code": String::from("0x") + &hex::encode(account.code.1.into_bytes()),
        });

        let mut state = BTreeMap::new();
        for key in kv.get(&address).cloned().unwrap_or_default() {
            let val = ext.get(&address, &key).await?;
            state.insert(key, val);
        }
        if !state.is_empty() {
            json["state"] = serde_json::to_value(state).unwrap();
        }
        ret.insert(address.to_string(), json);
    }
    Ok(ret)
}

pub fn runner(
    header: Header,
    ext: Ext,
) -> impl FnMut(Tx) -> Pin<Box<dyn Future<Output = eyre::Result<(TxResult, Vec<Event>, u64)>>>> {
    let ext = Arc::new(Mutex::new(ext));
    let base_fee = header.base_fee;
    move |tx| {
        let effective_gas_price = tx.effective_gas_price(base_fee);
        let calldata = tx.input.as_ref().to_vec();

        let call_cost = 21000i64;
        let data_cost = {
            let total_calldata_len = calldata.len();
            let nonzero_bytes_count = calldata.iter().filter(|byte| *byte != &0).count();
            nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
        } as i64;
        let create_cost = 32000i64;
        let init_code_cost = 2 * calldata.len().div_ceil(32) as i64;

        // EIP-7623: Increase calldata cost
        let calldata_tokens = {
            let zero_bytes = calldata.iter().filter(|b| **b == 0).count() as i64;
            let nonzero_bytes = calldata.len() as i64 - zero_bytes;
            zero_bytes + nonzero_bytes * 4
        };
        let gas_floor = 21000 + 10 * calldata_tokens;

        let header = header.clone();
        let ext = ext.clone();

        let tx_ctx = TxContext {
            gas_price: effective_gas_price,
            gas_max_fee: tx.gas_info.max_fee.unwrap_or_default(),
            gas_max_priority_fee: tx.gas_info.max_priority_fee.unwrap_or_default(),
            blob_max_fee: tx.gas_info.max_fee_per_blob.unwrap_or_default(),
            blob_gas_used: (tx.blob_count() * 131072) as u64,
            access_list: tx.access_list.clone(),
        };

        let access_list_cost = tx_ctx.access_list_cost();

        Box::pin(async move {
            let (tx_result, traces, ms) = tokio::spawn(async move {
                let mut guard = ext.lock().await;
                guard.reset(tx_ctx);

                let now = std::time::Instant::now();
                let mut result = Solenoid::new()
                    .execute(tx.to.unwrap_or_default(), "", tx.input.as_ref())
                    .with_header(header.clone())
                    .with_sender(tx.from)
                    .with_gas(tx.gas)
                    .with_value(tx.value)
                    .ready()
                    .apply(&mut *guard)
                    .await?;
                let ms = now.elapsed().as_millis() as u64;

                // let coinbase_balance = guard.balance(&header.miner).await?;
                // println!("[SOLE] COINBASE BALANCE: {coinbase_balance:#x}");

                let gas_costs = if tx.to.is_some() {
                    call_cost + data_cost + access_list_cost
                } else {
                    let deployed_code_cost = 200 * result.ret.len() as i64;
                    call_cost
                        + data_cost
                        + create_cost
                        + init_code_cost
                        + deployed_code_cost
                        + access_list_cost
                };

                let traces = result
                    .tracer
                    .take()
                    .into_iter()
                    .filter(|event| matches!(event.data, EventData::OpCode(_)))
                    .map(|mut event| {
                        if let EventData::OpCode(opcode) = &mut event.data {
                            opcode.stack.reverse();
                        }
                        event
                    })
                    .collect();

                let tx_result = as_tx_result(gas_costs, gas_floor, &result, &mut *guard).await?;

                Ok::<_, eyre::Report>((tx_result, traces, ms))
            })
            .await??;

            Ok((tx_result, traces, ms))
        })
    }
}

use solenoid::allocator::LoggingAllocator;
use std::alloc::System;

#[global_allocator]
static GLOBAL: LoggingAllocator<System> = LoggingAllocator(System);

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);

    // Fail fast if the RPC URL is invalid or unresponsive
    let chain_id = eth.chain_id().await?;
    if chain_id != 1 {
        eyre::bail!("Unexpected chain ID: {chain_id}");
    }

    let block_number = std::env::args().nth(1);
    let block_number = if block_number.as_ref().map(|s| s.as_str()) == Some("latest") {
        let (block_number, _) = eth.get_latest_block().await?;
        block_number
    } else {
        block_number
            .and_then(|number| number.parse::<u64>().ok())
            .unwrap_or(23027350) // https://xkcd.com/221/
    };

    let args = std::env::args().skip(1).collect::<HashSet<String>>();
    let progress = args.contains("--progress");
    let memory = args.contains("--memory");

    let Block {
        header,
        transactions,
    } = eth.get_full_block(Word::from(block_number)).await?;
    println!(
        "📦 Fetched block number: {} [{} txs]",
        header.number.as_usize(),
        transactions.len()
    );

    let ext = Ext::at_number(Word::from(block_number - 1), eth).await?;

    let mut f = runner(header, ext);

    let provider = ProviderBuilder::new().connect_http(url.parse()?);
    let block = match provider
        .get_block_by_number(BlockNumberOrTag::Number(block_number))
        .full()
        .await
    {
        Ok(Some(block)) => block,
        Ok(None) => eyre::bail!("Block not found"),
        Err(error) => eyre::bail!("Error: {:?}", error),
    };

    let BlockTransactions::Full(txs) = block.transactions else {
        eyre::bail!("Expected full block");
    };
    println!(
        "📦 Fetched block number: {} [{} txs]",
        block.header.number,
        txs.len()
    );

    let mut g = evm_tracer::run::runner(block.header, provider);

    let len = transactions.len();
    assert_eq!(txs.len(), len);

    let mut matched = 0;
    for idx in 0..len {
        let tx = txs[idx].clone();
        let (revm_result, revm_traces, revm_ms) = g(tx)?;

        let tx = transactions[idx].clone();
        let result = f(tx).await;

        match result {
            Ok((sole_result, sole_traces, sole_ms)) => {
                if progress {
                    use std::io::Write;
                    eprint!(
                        "\rTX: {idx:>3}/{:>3} | REVM: {revm_ms:>5} ms | SOLE: {sole_ms:>5} ms",
                        len - 1
                    );
                    std::io::stdout().flush().unwrap();
                }

                let rev_ok = revm_result.rev == sole_result.rev;
                let ret_ok = revm_result.ret == sole_result.ret;
                let gas_ok = revm_result.gas == sole_result.gas;
                let traces_ok = revm_traces == sole_traces;
                let state_ok = revm_result.state == sole_result.state;

                if memory {
                    let (used, diff) = solenoid::allocator::stats();
                    println!("MEMORY: {used:>10} \t {diff:>10}");
                }

                if rev_ok && ret_ok && gas_ok && traces_ok && state_ok {
                    matched += 1;
                    continue;
                }

                // Dump traces to files for later analysis
                let revm_trace_file = format!("revm.{}.{}.log", block_number, idx);
                let sole_trace_file = format!("sole.{}.{}.log", block_number, idx);
                evm_tracer::aux::dump(&revm_trace_file, &revm_traces).ok();
                let is_opcode = |e: &Event| matches!(e.data, EventData::OpCode(_));
                evm_tracer::aux::dump_filtered(&sole_trace_file, &sole_traces, is_opcode).ok();

                let revm_traces_len = revm_traces.len();
                drop(revm_traces);
                drop(sole_traces);

                // Dump state to files for later analysis (only serialize when needed)
                let revm_state_file = format!("revm.{}.{}.state.json", block_number, idx);
                let sole_state_file = format!("sole.{}.{}.state.json", block_number, idx);
                std::fs::write(
                    &revm_state_file,
                    serde_json::to_string_pretty(&revm_result.state).unwrap_or_default(),
                )
                .ok();
                std::fs::write(
                    &sole_state_file,
                    serde_json::to_string_pretty(&sole_result.state).unwrap_or_default(),
                )
                .ok();

                let ret = if revm_result.ret.is_empty() {
                    "empty".to_string()
                } else {
                    format!("<{}>", revm_result.ret.len())
                };
                println!(
                    "---\n### {block_number} {idx} hash={}",
                    txs[idx].info().hash.unwrap_or_default()
                );
                let state_accounts = revm_result.state.len();
                let state_keys = revm_result
                    .state
                    .iter()
                    .map(|(_, value)| {
                        value
                            .as_object()
                            .and_then(|object| object.get("state"))
                            .and_then(|v| v.as_object())
                            .map(|object| object.len())
                            .unwrap_or_default()
                    })
                    .sum::<usize>();
                println!(
                    "REVM \tOK={} \tRET={:4}\tGAS={}\tTRACES={:5<}\tSTATE={}+{}\t{revm_ms} ms",
                    !revm_result.rev,
                    ret,
                    revm_result.gas,
                    revm_traces_len,
                    state_accounts,
                    state_keys,
                );

                let ret_diff = if revm_result.ret == sole_result.ret {
                    "match".to_string()
                } else {
                    format!("<{}>", sole_result.ret.len())
                };
                let gas_diff = if revm_result.gas == sole_result.gas {
                    "match".to_string()
                } else {
                    format!("{:+5}", sole_result.gas - revm_result.gas)
                };
                let state_diff = if state_ok {
                    "match".to_string()
                } else {
                    let state_accounts = sole_result.state.len();
                    let state_keys = sole_result
                        .state
                        .iter()
                        .map(|(_, value)| {
                            value
                                .get("state")
                                .and_then(|v| v.as_object())
                                .map(|object| object.len())
                                .unwrap_or_default()
                        })
                        .sum::<usize>();
                    format!("{}+{}", state_accounts, state_keys)
                };
                println!(
                    "sole \tOK={} \tRET={}\tGAS={}\tTRACES={:5<}\tSTATE={}\t{sole_ms} ms",
                    !sole_result.rev,
                    ret_diff,
                    gas_diff,
                    if traces_ok { "match" } else { "false" },
                    state_diff,
                );

                drop(revm_result);
                drop(sole_result);
            }
            Err(e) => {
                println!(
                    "---\n### {block_number} {idx} hash={}",
                    txs[idx].info().hash.unwrap_or_default()
                );
                println!(
                    "REVM \tOK={} \tRET={:4}\tGAS={}\tTRACES={:5<}",
                    !revm_result.rev,
                    true,
                    revm_result.gas,
                    revm_traces.len()
                );
                println!("sole \tPANIC: '{e}'");
            }
        }
    }

    println!(
        "\n(total: {len}, matched: {matched}, invalid: {})",
        len - matched
    );
    Ok(())
}
