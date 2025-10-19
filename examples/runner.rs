use std::time::Instant;
use std::{pin::Pin, sync::Arc};

use evm_tracer::alloy_eips::BlockNumberOrTag;
use evm_tracer::alloy_provider::{Provider, ProviderBuilder};
use evm_tracer::alloy_rpc_types::BlockTransactions;
use tokio::sync::Mutex;

use solenoid::{common::{block::Block, word::Word}, eth, ext::Ext};
use solenoid::{
    common::block::{Header, Tx},
    solenoid::{Builder as _, CallResult, Solenoid},
    tracer::{EventTracer as _, LoggingTracer},
};

use evm_tracer::{OpcodeTrace, run::TxResult};

fn as_tx_result(gas_costs: i64, value: CallResult<LoggingTracer>) -> TxResult {
    TxResult {
        gas: value.evm.gas.finalized(gas_costs) as u64,
        ret: value.ret,
        rev: value.evm.reverted,
    }
}

pub fn runner(
    header: Header,
    ext: Ext,
) -> impl FnMut(Tx) -> Pin<Box<dyn Future<Output = eyre::Result<(TxResult, Vec<OpcodeTrace>)>>>> {
    let ext = Arc::new(Mutex::new(ext));
    move |tx| {
        let call_cost = 21000i64;
        let data_cost = {
            let total_calldata_len = tx.input.as_ref().len();
            let nonzero_bytes_count =
                tx.input.as_ref().iter().filter(|byte| *byte != &0).count();
            nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
        } as i64;
        let create_cost = 32000i64;
        let init_code_cost = 2 * tx.input.as_ref().len().div_ceil(32) as i64;

        let header = header.clone();
        let ext = ext.clone();
        Box::pin(async move {
            let mut result = tokio::spawn(async move {
                let mut guard = ext.lock().await;
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
            }).await??;

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

            Ok((as_tx_result(gas_costs, result), traces))
        })
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);

    let block_number = std::env::args().nth(1);
    let block_number = if block_number.as_ref().map(|s| s.as_str()) == Some("latest") {
        let (block_number, _) = eth.get_latest_block().await?;
        block_number
    } else {
        block_number
            .and_then(|number| number.parse::<u64>().ok())
            .unwrap_or(23027350) // https://xkcd.com/221/
    };

    let Block{ header, transactions } = eth.get_full_block(Word::from(block_number)).await?;
    println!("📦 Fetched block number: {} [with {} txs]", header.number.as_usize(), transactions.len());

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
    println!("📦 Fetched block number: {} [with {} txs]", block.header.number, txs.len());

    let mut g = evm_tracer::run::runner(block.header, provider);

    let len = transactions.len();
    assert_eq!(txs.len(), len);

    for idx in 0..len {
        println!("---\n### block={block_number} index={idx} hash={}", txs[idx].info().hash.unwrap_or_default());

        let tx = txs[idx].clone();
        let now = Instant::now();
        let (result, traces) = g(tx)?;
        let ms = now.elapsed().as_millis();
        println!("REVM \tOK={} \tRET={} \tGAS={} \tTRACES={} \tms={ms}", 
            !result.rev,
            true,
            result.gas,
            traces.len());
        let ret = result.ret;

        let tx = transactions[idx].clone();
        let now = Instant::now();
        let r = f(tx).await;
        let ms = now.elapsed().as_millis();
        match r {
            Ok((result, traces)) => {
                println!("sole \tOK={} \tRET={} \tGAS={} \tTRACES={} \tms={ms}", 
                    !result.rev,
                    result.ret == ret,
                    result.gas,
                    traces.len());
            }
            Err(e) => {
                println!("sole \tPANIC: '{e}'"); 
            }
        }
    }

    Ok(())
}
