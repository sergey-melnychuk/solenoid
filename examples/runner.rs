use std::time::Instant;
use std::{pin::Pin, sync::Arc};

use tokio::sync::Mutex;

use solenoid::{common::{block::Block, word::Word}, eth, ext::Ext};
use solenoid::{
    common::block::{Header, Tx},
    solenoid::{Builder as _, CallResult, Solenoid},
    tracer::{EventTracer as _, LoggingTracer},
};

use evm_tracer::{OpcodeTrace, run::TxResult};

fn as_tx_result(value: CallResult<LoggingTracer>) -> TxResult {
    TxResult {
        gas: value.evm.gas.used as u64, // TODO: use finalized gas
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

            let traces = result
                .tracer
                .take()
                .into_iter()
                .filter_map(|event| evm_tracer::OpcodeTrace::try_from(event).ok())
                .collect::<Vec<_>>();

            Ok((as_tx_result(result), traces))
        })
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    let _ = tracing_subscriber::fmt::try_init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);

    let block_number = std::env::args()
        .nth(1)
        .and_then(|number| number.parse::<u64>().ok())
        .unwrap_or(23027350); // https://xkcd.com/221/

    let Block{ header, transactions } = eth.get_full_block(Word::from(block_number)).await?;
    eprintln!("📦 Fetched block number: {} [with {} txs]", header.number.as_usize(), transactions.len());

    let ext = Ext::at_number(Word::from(block_number - 1), eth).await?;

    let mut f = runner(header, ext);

    for tx in transactions {
        let idx = tx.index.as_usize();
        let now = Instant::now();
        match f(tx).await {
            Ok((result, traces)) => {
                let ms = now.elapsed().as_millis();
                eprintln!("TX \tindex={idx} \tOK={} \tGAS={:03} \tTRACES={:03} \tms={ms}", 
                    !result.rev, 
                    result.gas,
                    traces.len());
            }
            Err(e) => {
                eprintln!("TX \tindex={idx} \tPANIC: '{e}'"); 
            }
        }
    }

    Ok(())
}