use evm_tracer::alloy_eips::BlockNumberOrTag;
use evm_tracer::alloy_provider::network::primitives::BlockTransactions;
use evm_tracer::alloy_provider::{Provider, ProviderBuilder};
use evm_tracer::eyre::{self, Result};
use solenoid::common::hash;
use solenoid::common::word::Word;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let url = std::env::var("URL")?.parse()?;
    let client = ProviderBuilder::new().connect_http(url);

    let block_number = std::env::args()
        .nth(1)
        .and_then(|number| number.parse::<u64>().ok())
        .unwrap_or(23027350);

    let skip = std::env::args()
        .nth(2)
        .and_then(|number| number.parse::<usize>().ok())
        .unwrap_or(0);

    let block = match client
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
    eprintln!("📦 Fetched block number: {}", block.header.number);

    let txs = txs.into_iter();
    let txs = txs.skip(skip).take(1);
    let traced = evm_tracer::trace_all(txs, &block.header, &client).await?;
    for (result, traces) in traced {
        let ret = result.result.output().unwrap_or_default().as_ref();
        eprintln!("---");
        if ret.len() <= 512 {
            eprintln!("RET: {}", hex::encode(ret));
        } else {
            eprintln!("RET: len={} hash={}", ret.len(), Word::from_bytes(&hash::keccak256(ret)));
        }
        eprintln!("GAS: {}", result.result.gas_used());
        eprintln!("OK: {}", !result.result.is_halt());

        let path = format!("revm.{block_number}.{skip}.log");
        evm_tracer::aux::dump(&path, &traces.traces)?;
        println!("TRACES: {} in {path}", traces.traces.len());
    }
    Ok(())
}
