use std::collections::{BTreeMap, BTreeSet};
use std::{pin::Pin, sync::Arc};

use evm_tracer::alloy_eips::BlockNumberOrTag;
use evm_tracer::alloy_provider::{Provider, ProviderBuilder};
use evm_tracer::alloy_rpc_types::BlockTransactions;
use serde_json::Value;
use solenoid::common::address::Address;
use solenoid::ext::TxContext;
use tokio::sync::Mutex;

use solenoid::{
    common::block::{Header, Tx},
    solenoid::{Builder as _, CallResult, Solenoid},
    tracer::{EventTracer as _, LoggingTracer},
};
use solenoid::{
    common::{block::Block, word::Word},
    eth,
    ext::Ext,
};

use evm_tracer::{OpcodeTrace, run::TxResult};

async fn as_tx_result(gas_costs: i64, gas_floor: i64, result: &CallResult<LoggingTracer>, ext: &mut Ext) -> eyre::Result<TxResult> {
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

async fn as_state(result: &CallResult<LoggingTracer>, ext: &mut Ext) -> eyre::Result<BTreeMap<String, Value>> {
    let mut kv: BTreeMap<Address, BTreeSet<Word>> = BTreeMap::new();
    let touched = result.evm.touches
        .iter()
        .filter_map(|touch| match touch {
            // solenoid::executor::AccountTouch::WarmUp(address) => {
            //     Some(*address)
            // }
            // solenoid::executor::AccountTouch::GetNonce(address, _) => {
            //     Some(*address)
            // }
            // solenoid::executor::AccountTouch::GetValue(address, _) => {
            //     Some(*address)
            // }
            solenoid::executor::AccountTouch::GetState(address, key, _, _) => {
                kv.entry(*address).or_default().insert(*key);
                Some(*address)
            }
            solenoid::executor::AccountTouch::GetCode(address, _, _) => {
                Some(*address)
            }
            // touches that modify the state:
            solenoid::executor::AccountTouch::FeePay(address, _, _) => {
                Some(*address)
            }
            solenoid::executor::AccountTouch::SetState(address, key, _, _, _) => {
                kv.entry(*address).or_default().insert(*key);
                Some(*address)
            }
            solenoid::executor::AccountTouch::SetNonce(address, _, _) => {
                Some(*address)
            }
            solenoid::executor::AccountTouch::SetValue(address, _, _) => {
                Some(*address)
            }
            solenoid::executor::AccountTouch::Create(address, _, _, _, _) => {
                Some(*address)
            }
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
) -> impl FnMut(Tx) -> Pin<Box<dyn Future<Output = eyre::Result<(TxResult, Vec<OpcodeTrace>)>>>> {
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
            let (tx_result, traces) = tokio::spawn(async move {
                let mut guard = ext.lock().await;
                guard.reset(tx_ctx);

                let mut result = Solenoid::new()
                    .execute(tx.to.unwrap_or_default(), "", tx.input.as_ref())
                    .with_header(header.clone())
                    .with_sender(tx.from)
                    .with_gas(tx.gas)
                    .with_value(tx.value)
                    .ready()
                    .apply(&mut *guard)
                    .await?;

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
                    .filter_map(|event| evm_tracer::OpcodeTrace::try_from(event).ok())
                    .collect::<Vec<_>>();
    
                let tx_result = as_tx_result(gas_costs, gas_floor, &result, &mut *guard).await?;

                Ok::<_, eyre::Report>((tx_result, traces))
            })
            .await??;

            Ok((tx_result, traces))
        })
    }
}

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

    let progress = std::env::args().skip(2).any(|arg| arg == "--progress");

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
        if progress {
            use std::io::Write;
            eprint!("\rTX: {idx:>3}/{:>3}", len - 1);
            std::io::stdout().flush().unwrap();
        }

        let tx = txs[idx].clone();
        let (revm_result, revm_traces) = g(tx)?;

        let tx = transactions[idx].clone();
        let result = f(tx).await;
        match result {
            Ok((sole_result, sole_traces)) => {
                let rev_ok = revm_result.rev == sole_result.rev;
                let ret_ok = revm_result.ret == sole_result.ret;
                let gas_ok = revm_result.gas == sole_result.gas;
                let traces_ok = revm_traces == sole_traces;

                let revm_state = serde_json::to_string_pretty(&revm_result.state).unwrap();
                let sole_state = serde_json::to_string_pretty(&sole_result.state).unwrap();
                let state_ok = sole_state == revm_state;

                let ok = rev_ok && ret_ok && gas_ok && traces_ok && state_ok;
                if ok {
                    matched += 1;
                    continue;
                }

                // Dump traces to files for later analysis
                let revm_trace_file = format!("revm.{}.{}.log", block_number, idx);
                let sole_trace_file = format!("sole.{}.{}.log", block_number, idx);

                std::fs::write(
                    &revm_trace_file,
                    revm_traces
                        .iter()
                        .map(|t| serde_json::to_string(t).unwrap())
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
                .ok();

                std::fs::write(
                    &sole_trace_file,
                    sole_traces
                        .iter()
                        .map(|t| serde_json::to_string(t).unwrap())
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
                .ok();

                // Dump state to files for later analysis
                let revm_state_file = format!("revm.{}.{}.state.json", block_number, idx);
                let sole_state_file = format!("sole.{}.{}.state.json", block_number, idx);
                std::fs::write(
                    &revm_state_file,
                    &revm_state
                ).ok();
                std::fs::write(
                    &sole_state_file,
                    &sole_state
                ).ok();

                let ret = if revm_result.ret.is_empty() {
                    "empty".to_string()
                } else {
                    format!("<{}>", revm_result.ret.len())
                };
                println!(
                    "---\n### {block_number} {idx} hash={}",
                    txs[idx].info().hash.unwrap_or_default()
                );
                println!(
                    "REVM \tOK={} \tRET={:4}\tGAS={}\tTRACES={}\tSTATE={}",
                    !revm_result.rev,
                    ret,
                    revm_result.gas,
                    revm_traces.len(),
                    revm_result.state.len()
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
                println!(
                    "sole \tOK={} \tRET={}\tGAS={}\tTRACES={}\tSTATE={}",
                    !sole_result.rev,
                    ret_diff,
                    gas_diff,
                    sole_traces.len(),
                    state_ok,
                );
            }
            Err(e) => {
                println!(
                    "---\n### {block_number} {idx} hash={}",
                    txs[idx].info().hash.unwrap_or_default()
                );
                println!(
                    "REVM \tOK={} \tRET={:4}\tGAS={}\tTRACES={}",
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
