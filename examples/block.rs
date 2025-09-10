use std::{
    panic::AssertUnwindSafe,
    sync::{LazyLock, Mutex},
    time::Instant,
};

use eyre::{Context, OptionExt, eyre};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use solenoid::{
    common::{
        Hex,
        address::Address,
        word::{Word, decode_error_string},
    },
    eth,
    ext::Ext,
    solenoid::{Builder, Solenoid},
};

static PANIC_MESSAGE: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));

fn set_panic_message(msg: String) {
    *PANIC_MESSAGE.lock().unwrap() = Some(msg);
}

fn get_panic_message() -> Option<String> {
    PANIC_MESSAGE.lock().unwrap().take()
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s
        } else {
            "undefined"
        };
        set_panic_message(msg.to_string());
    }));

    dotenv::dotenv().ok();
    let _ = tracing_subscriber::fmt::try_init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);

    // let (number, _) = eth.get_latest_block().await?;
    let number = 23027350;

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
    let (mut seq, mut ok, mut rev, mut failed, mut panic) = (0, 0, 0, 0, 0);
    for tx in &txs {
        seq += 1;
        let idx = tx.index.as_u64();
        println!("---\nTX {idx}: {}", tx.hash);
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
            .map_err(|_| eyre!("panic-caught"))
            .with_context(|| format!("TX:{idx}:{}", tx.hash));
        let ms = now.elapsed().as_millis();
        match result {
            Ok(result) => match result {
                Ok(result) => {
                    let ret = hex::encode(&result.ret);
                    if !result.evm.reverted {
                        ok += 1;
                        println!("TX {idx}: OK: 0x{ret} (in {ms} ms)");
                    } else {
                        rev += 1;
                        let msg = decode_error_string(&result.ret)
                            .map(|msg| format!("\'{msg}\'"))
                            .unwrap_or_else(|| format!("0x{ret}"));
                        println!("TX {idx}: REVERT: {msg} (in {ms} ms)");
                    }
                }
                Err(e) => {
                    failed += 1;
                    println!("TX {idx}: FAILED: {e:?} (in {ms} ms)");
                }
            },
            Err(_) => {
                panic += 1;
                let msg = get_panic_message().unwrap_or_else(|| "undefined".to_string());
                println!("TX {idx}: PANIC: '{msg}' (in {ms} ms)");
            }
        };
    }

    assert_eq!(txs.len(), seq);
    println!("---\nOK: {ok}, REVERT: {rev}, FAILED: {failed}, PANIC: {panic}");
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

// Data for block 23027350:
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
