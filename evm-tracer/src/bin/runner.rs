use std::time::Instant;

use alloy_eips::BlockNumberOrTag;
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::BlockTransactions;
use evm_tracer::run::runner;
use eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let url = std::env::var("URL")?.parse()?;
    let provider = ProviderBuilder::new().connect_http(url);

    let block_number = std::env::args()
        .nth(1)
        .and_then(|number| number.parse::<u64>().ok())
        .unwrap_or(23027350); // https://xkcd.com/221/
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
    eprintln!("📦 Fetched block number: {} [with {} txs]", block.header.number, txs.len());

    let mut f = runner(block.header, provider);
    for tx in txs {
        let idx = tx.transaction_index.unwrap_or_default();
        let now = Instant::now();
        let (result, tracer) = f(tx)?;
        let ms = now.elapsed().as_millis();
        eprintln!("TX \tindex={idx} \tOK={} \tGAS={} \tTRACES={} \tms={ms}", 
            result.result.is_success(), 
            result.result.gas_used(),
            tracer.traces.len());
    }
    
    Ok(())
}