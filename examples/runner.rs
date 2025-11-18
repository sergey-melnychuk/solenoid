use std::io::Write;
use std::{pin::Pin, sync::Arc};

use evm_tracer::alloy_eips::BlockNumberOrTag;
use evm_tracer::alloy_provider::{Provider, ProviderBuilder};
use evm_tracer::alloy_rpc_types::BlockTransactions;
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

fn as_tx_result(gas_costs: i64, gas_floor: i64, result: CallResult<LoggingTracer>) -> TxResult {
    let gas = result
        .evm
        .gas
        .finalized(gas_costs, result.evm.reverted)
        .max(gas_floor);
    TxResult {
        gas,
        ret: result.ret,
        rev: result.evm.reverted,
    }
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
        Box::pin(async move {
            let mut result = tokio::spawn(async move {
                let mut guard = ext.lock().await;
                guard.reset(effective_gas_price, tx.max_fee_per_gas.unwrap_or_default(), tx.max_priority_fee_per_gas.unwrap_or_default());
                let result = Solenoid::new()
                    .execute(tx.to.unwrap_or_default(), "", tx.input.as_ref())
                    .with_header(header.clone())
                    .with_sender(tx.from)
                    .with_gas(tx.gas)
                    .with_value(tx.value)
                    .ready()
                    .apply(&mut *guard)
                    .await?;
                Ok::<_, eyre::Report>(result)
            })
            .await??;

            let gas_costs = if tx.to.is_some() {
                call_cost + data_cost
            } else {
                let deployed_code_cost = 200 * result.ret.len() as i64;
                call_cost + data_cost + create_cost + init_code_cost + deployed_code_cost
            };

            let traces = result
                .tracer
                .take()
                .into_iter()
                .filter_map(|event| evm_tracer::OpcodeTrace::try_from(event).ok())
                .collect::<Vec<_>>();

            Ok((as_tx_result(gas_costs, gas_floor, result), traces))
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

    let Block {
        header,
        transactions,
    } = eth.get_full_block(Word::from(block_number)).await?;
    println!(
        "📦 Fetched block number: {} [with {} txs]",
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
        "📦 Fetched block number: {} [with {} txs]",
        block.header.number,
        txs.len()
    );

    let mut g = evm_tracer::run::runner(block.header, provider);

    let len = transactions.len();
    assert_eq!(txs.len(), len);

    let mut matched = 0;
    for idx in 0..len {
        eprint!("\rTX: {idx:>3}/{:>3}", len - 1);
        std::io::stdout().flush().unwrap();

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

                let ok = rev_ok && ret_ok && gas_ok && traces_ok;
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
                    "REVM \tOK={} \tRET={:4}\tGAS={}\tTRACES={}",
                    !revm_result.rev,
                    ret,
                    revm_result.gas,
                    revm_traces.len()
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
                    "sole \tOK={} \tRET={}\tGAS={}\tTRACES={}",
                    !sole_result.rev,
                    ret_diff,
                    gas_diff,
                    sole_traces.len()
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
