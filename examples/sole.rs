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
        ext.pull(&tx.from).await?;
        ext.acc_mut(&tx.from).value = Word::from_hex("0x90a4a345dbae6ead").unwrap();
        // eprintln!("TX: {}", tx.hash);
        // eprintln!("GAS PRICE: {}", tx.gas_price.as_u64());
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
        eprintln!("---\nRET: {}", hex::encode(&result.ret));

        // Calculate transaction costs like in executor.rs:302-312
        let call_cost = 21000u64;
        let data_cost = {
            let total_calldata_len = tx.input.as_ref().len();
            let nonzero_bytes_count = tx.input.as_ref().iter().filter(|byte| *byte != &0).count();
            nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
        };
        let total_tx_cost = call_cost + data_cost as u64;
        let final_gas_with_tx_cost = result.evm.gas.finalized().as_u64() + total_tx_cost;
        // eprintln!("DEBUG: tx_cost={}, execution_gas={}, refunded_gas={}, final_total={}",
        //           total_tx_cost, result.evm.gas.used.as_u64(), result.evm.gas.refund.as_u64(), final_gas_with_tx_cost);
        eprintln!("GAS: {}", final_gas_with_tx_cost);

        eprintln!("OK: {}", !result.evm.reverted);
        for tr in traces {
            println!("{}", serde_json::to_string(&tr).expect("json"));
        }
    }
    Ok(())
}
