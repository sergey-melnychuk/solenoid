use alloy_eips::BlockNumberOrTag;
use alloy_provider::network::primitives::BlockTransactions;
use alloy_provider::{Provider, ProviderBuilder};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let url = std::env::var("URL")?.parse()?;
    let client = ProviderBuilder::new().connect_http(url);

    let block_number = std::env::args()
        .nth(1)
        .and_then(|number| number.parse::<u64>().ok())
        .unwrap_or(23313036); // https://xkcd.com/221/
    let block = match client
        .get_block_by_number(BlockNumberOrTag::Number(block_number))
        .full()
        .await
    {
        Ok(Some(block)) => block,
        Ok(None) => anyhow::bail!("Block not found"),
        Err(error) => anyhow::bail!("Error: {:?}", error),
    };

    let BlockTransactions::Full(txs) = block.transactions else {
        anyhow::bail!("Expected full block");
    };
    eprintln!("📦 Fetched block number: {}", block.header.number);

    let txs = txs.into_iter();
    // Trace single transaction just for the sake of speed and simplicity
    let txs = txs.take(1);
    let res = evm_tracer::trace_all(txs, &block.header, &client).await?;

    for (result, traces) in res {
        let json = serde_json::to_string_pretty(&(traces, result))?;
        println!("---\n{json}");
    }
    Ok(())
}
