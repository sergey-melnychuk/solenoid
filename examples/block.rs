use std::{panic::AssertUnwindSafe, time::Instant};

use eyre::{Context, OptionExt, eyre};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use solenoid::{
    common::{Hex, address::Address, word::Word},
    eth,
    ext::Ext,
    solenoid::{Builder, Solenoid},
};

// RUST_LOG=off cargo run --example block

fn get_panic_message(any: &dyn std::any::Any) -> String {
    if let Some(s) = any.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = any.downcast_ref::<String>() {
        s.to_owned()
    } else {
        "undefined".to_string()
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // std::panic::set_hook(Box::new(|info| {
    //     eprintln!("PANIC: {}.", get_panic_message(info.payload()));
    // }));

    dotenv::dotenv().ok();
    let _ = tracing_subscriber::fmt::try_init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);

    // let (number, _) = eth.get_latest_block().await?;
    let number = 0x15f5e96;

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

    println!("BLOCK: {number}");
    for tx in txs {
        println!("---\nTX {}: {}", tx.index, tx.hash);
        let now = Instant::now();
        let result = Solenoid::new()
            .execute(tx.to.unwrap_or_else(Address::zero), "", tx.input.as_ref())
            .with_sender(tx.from)
            .with_gas(tx.gas)
            .with_value(tx.value)
            .ready()
            .apply(&mut ext);
        let result = AssertUnwindSafe(result)
            .catch_unwind()
            .await
            .map_err(|e| eyre!("{}", get_panic_message(&e)))
            .with_context(|| format!("TX:{}:{}", tx.index, tx.hash));
        let ms = now.elapsed().as_millis();
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                println!("TX {}: PANIC: {} (in {ms} ms)", tx.index, e);
                continue;
            }
        };
        match result {
            Ok(result) => {
                println!(
                    "TX {}: OK: 0x{} (in {ms} ms)",
                    tx.index,
                    hex::encode(result.ret)
                );
            }
            Err(e) => {
                println!("TX {}: FAILED: {} (in {ms} ms)", tx.index, e.to_string());
            }
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

/*

todo!("BLOCKHASH")
"hash": "0x5f98c4bb41ad348ee23e9f9f59eef495831da7d5e9c0975cde2d130896eb4824",

todo!("NUMBER")
"number": "0x15f5e96",

todo!("BASEFEE")
"baseFeePerGas": "0x5109a1b1",

todo!("PREVRANDAO")
"mixHash": "0x1d40a92c6d4359619a0b942c84fb60aae000fc5259e316d27aa3021fe735bb50",

todo!("TIMESTAMP")
"timestamp": "0x6889371b",

todo!("GASLIMIT")
"gasLimit": "0x2aea540",

todo!("BLOBHASH")
"extraData": "0x4275696c6465724e65742028466c617368626f747329",

todo!("GASPRICE")
"gasPrice": "0x3cf1b77b1", // TX

todo!("CHAINID")
"chainId": "0x1", // TX

todo!("BLOBBASEFEE")
???

todo!("COINBASE")
???

*/

#[allow(dead_code)]
struct Block {
    transactions: Vec<Tx>,
}
