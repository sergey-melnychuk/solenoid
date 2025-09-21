use eyre::{Context, eyre};
use solenoid::{
    common::word::Word,
    eth,
    ext::Ext,
    solenoid::{Builder, Solenoid},
    tracer::EventTracer,
};

// RUST_LOG=off cargo run --release --example sole > sole.log

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    let _ = tracing_subscriber::fmt::try_init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);

    // let (number, _) = eth.get_latest_block().await?;
    let number = 23027350; // 0x15f5e96

    let block = eth.get_full_block(Word::from(number)).await?;

    let mut ext = Ext::at_number(Word::from(number - 1), eth).await?;

    eprintln!("📦 Fetched block number: {number}");
    let txs = block.transactions.iter();
    let txs = txs.take(1);
    for tx in txs {
        let idx = tx.index.as_u64();
        let to = tx.to.unwrap_or_default();
        ext.pull(&tx.from).await?;
        ext.acc_mut(&tx.from).value = Word::from_hex("0x90a4a345dbae6ead").unwrap();
        ext.pull(&to).await?;
        ext.acc_mut(&to).value = Word::from_hex("0x90a4a345dbae6ead").unwrap();
        //eprintln!("TX: {tx:#?}");
        let mut result = Solenoid::new()
            .execute(tx.to.unwrap_or_default(), "", tx.input.as_ref())
            .with_header(block.header.clone())
            .with_sender(tx.from)
            .with_gas(tx.gas)
            .with_value(tx.value)
            .ready()
            .apply(&mut ext)
            .await
            .map_err(|_| eyre!("panic-caught"))
            .with_context(|| format!("TX:{idx}:{}", tx.hash))?;
        let traces = result
            .tracer
            .take()
            .into_iter()
            .filter_map(|event| evm_tracer::OpcodeTrace::try_from(event).ok())
            .collect::<Vec<_>>();
        for tr in traces {
            println!("{}", serde_json::to_string(&tr).expect("json"));
        }
    }
    Ok(())
}
