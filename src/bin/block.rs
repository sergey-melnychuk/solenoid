use eyre::{Context, OptionExt, eyre};
use serde::{Deserialize, Serialize};
use solenoid::{
    common::{
        Hex,
        address::Address,
        word::Word,
    },
    eth,
    ext::Ext,
    solenoid::{Builder, Solenoid},
    tracer::EventTracer,
};

// RUST_LOG=off cargo run --release --bin block > block.log

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    let _ = tracing_subscriber::fmt::try_init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);

    // let (number, _) = eth.get_latest_block().await?;
    let number = 23027350; // 0x15f5e96

    let txs = eth
        .get_full_block(Word::from(number), |json| {
            let txs = json
                .get("transactions")
                .cloned()
                .ok_or_eyre("no transactions")?;
            let txs: Vec<Tx> = serde_json::from_value(txs)?;
            Ok(txs)
        })
        .await?;

    let mut ext = Ext::at_number(Word::from(number - 1), eth).await?;

    eprintln!("📦 Fetched block number: {number}");
    let txs = txs.into_iter().take(1);
    for tx in txs {
        let idx = tx.index.as_u64();
        ext.acc_mut(&tx.from).value = Word::from_hex("0x90a4a345dbae6ead").unwrap();
        let mut result = Solenoid::new()
            .execute(tx.to.unwrap_or_else(Address::zero), "", tx.input.as_ref())
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
            println!("{}", serde_json::to_string_pretty(&tr).expect("json"));
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct Tx {
    hash: Word,
    #[serde(rename = "transactionIndex")]
    index: Word,
    from: Address,
    gas: Word,
    input: Hex,
    to: Option<Address>,
    value: Word,
}

#[allow(dead_code)]
struct Block {
    transactions: Vec<Tx>,
}
