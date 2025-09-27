use eyre::{Context, eyre};
use solenoid::{
    common::{address::addr, hash, word::{word, Word}},
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
    let txs = txs.skip(4).take(1);
    for tx in txs {
        let idx = tx.index.as_u64();

        // TX:1
        ext.pull(&tx.from).await?;
        ext.acc_mut(&tx.from).value = Word::from_hex("0x90a4a345dbae6ead").unwrap();

        // TX:2
        let acc = addr("0x042523db4f3effc33d2742022b2490258494f8b3");
        ext.pull(&acc).await?;
        ext.acc_mut(&acc).value = word("0x7af6c7f2729115eee");

        // TX:3
        let acc = addr("0x0fc7cb62247151faf5e7a948471308145f020d2e");
        ext.pull(&acc).await?;
        ext.acc_mut(&acc).value = word("0x7af6c7f2728a1bef0");

        // TX:4
        let acc = addr("0x8a14ce0fecbefdcc612f340be3324655718ce1c1");
        ext.pull(&acc).await?;
        ext.acc_mut(&acc).value = word("0x7af6c7f2728a0e4f0");

        // eprintln!("TX: {tx:#?}");
        // eprintln!("TX hash: {}", tx.hash);
        // eprintln!("GAS PRICE: {}", tx.gas_price.as_u64());
        eprintln!("GAS LIMIT: {}", tx.gas.as_u64());

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
        eprintln!("---");
        if result.ret.len() <= 512 {
            eprintln!("RET: {}", hex::encode(&result.ret));
        } else {
            eprintln!("RET: len={} hash={}", result.ret.len(), Word::from_bytes(&hash::keccak256(&result.ret)));
        }

        // Calculate transaction costs like in executor.rs:302-312
        let call_cost = 21000i64;
        let data_cost = {
            let total_calldata_len = tx.input.as_ref().len();
            let nonzero_bytes_count = tx.input.as_ref().iter().filter(|byte| *byte != &0).count();
            nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
        };
        let total_tx_cost = call_cost + data_cost as i64;
        let final_gas_with_tx_cost = result.evm.gas.finalized() + total_tx_cost;
        // eprintln!("DEBUG: tx_cost={}, execution_gas={}, refunded_gas={}, final_total={}",
        //           total_tx_cost, result.evm.gas.used, result.evm.gas.refund, final_gas_with_tx_cost);
        eprintln!("GAS: {}", final_gas_with_tx_cost);

        eprintln!("OK: {}", !result.evm.reverted);
        for tr in traces {
            println!("{}", serde_json::to_string(&tr).expect("json"));
        }
    }
    Ok(())
}
