use eyre::{Context, eyre};
use solenoid::{
    common::{address::Address, hash, word::Word},
    eth,
    ext::{Ext, TxContext},
    solenoid::{Builder, Solenoid},
    tracer::EventTracer,
};

#[allow(dead_code)]
async fn patch(ext: &mut Ext, acc: &Address, val: &str) -> eyre::Result<()> {
    ext.pull(&acc).await?;
    let old = ext.account_mut(&acc).value;
    let val = Word::from_hex(val)?;
    ext.account_mut(&acc).value = val;
    eprintln!("PATCH: {acc} balance {old} -> {val}");
    Ok(())
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

    let skip = std::env::args()
        .nth(2)
        .and_then(|number| number.parse::<usize>().ok())
        .unwrap_or(0);

    let block = eth.get_full_block(Word::from(block_number)).await?;

    let mut ext = Ext::at_number(Word::from(block_number - 1), eth).await?;

    eprintln!("📦 Fetched block number: {block_number}");
    let txs = block.transactions.iter();
    let txs = txs.skip(skip).take(1);
    for tx in txs {
        let idx = tx.index.as_u64();
        eprintln!("TX hash={:#064x} index={}", tx.hash, tx.index.as_usize());

        let tx_ctx = TxContext {
            gas_price: tx.effective_gas_price(block.header.base_fee),
            gas_max_fee: tx.gas_info.max_fee.unwrap_or_default(),
            gas_max_priority_fee: tx.gas_info.max_priority_fee.unwrap_or_default(),
            blob_max_fee: tx.gas_info.max_fee_per_blob.unwrap_or_default(),
            blob_gas_used: (tx.blob_count() * 131072) as u64,
            access_list: tx.access_list.clone(),
        };
        ext.reset(tx_ctx);
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
        let events = result.tracer.take();
        let len = result.tracer.peek().len();
        let mut traces = Vec::with_capacity(len);
        let mut i = 0;
        for event in events {
            if i % 1000 == 0 { use std::io::Write; print!("\r(mapping: {i} / {len})"); std::io::stdout().flush().unwrap(); }
            if let Ok(trace) = evm_tracer::OpcodeTrace::try_from(event) {
                traces.push(trace);
            }
            i += 1;
        }
        println!();
        eprintln!("---");
        if result.ret.len() <= 512 {
            eprintln!("RET: {}", hex::encode(&result.ret));
        } else {
            eprintln!(
                "RET: len={} hash={}",
                result.ret.len(),
                Word::from_bytes(&hash::keccak256(&result.ret))
            );
        }

        // EIP-7623: Increase calldata cost
        // Calculate tokens in calldata and floor gas
        let calldata_tokens = {
            let zero_bytes = tx.input.as_ref().iter().filter(|b| **b == 0).count() as i64;
            let nonzero_bytes = tx.input.as_ref().len() as i64 - zero_bytes;
            zero_bytes + nonzero_bytes * 4
        };
        let gas_floor = 21000 + 10 * calldata_tokens;

        let access_list_cost = ext.tx_ctx.access_list_cost();

        if tx.to.is_some() {
            let call_cost = 21000i64;
            let data_cost = {
                let total_calldata_len = tx.input.as_ref().len();
                let nonzero_bytes_count =
                    tx.input.as_ref().iter().filter(|byte| *byte != &0).count();
                nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
            } as i64;
            let total_gas = result.evm.gas.finalized(
                call_cost + data_cost + access_list_cost,
                result.evm.reverted,
            );
            let total_gas = total_gas.max(gas_floor);

            eprintln!("GAS: {total_gas}");
        } else {
            /*
            (See: https://www.evm.codes/?fork=cancun#f0)

            minimum_word_size = (size + 31) / 32
            init_code_cost = 2 * minimum_word_size
            code_deposit_cost = 200 * deployed_code_size

            static_gas = 32000
            dynamic_gas = init_code_cost
                + memory_expansion_cost
                + deployment_code_execution_cost
                + code_deposit_cost
            */

            let call_cost = 21000i64;
            let data_cost = {
                let total_calldata_len = tx.input.as_ref().len();
                let nonzero_bytes_count =
                    tx.input.as_ref().iter().filter(|byte| *byte != &0).count();
                nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
            } as i64;
            let create_cost = 32000i64;
            let init_code_cost = 2 * tx.input.as_ref().len().div_ceil(32) as i64;
            let deployed_code_cost = 200 * result.ret.len() as i64;

            let total_gas = result.evm.gas.finalized(
                call_cost
                    + data_cost
                    + create_cost
                    + init_code_cost
                    + deployed_code_cost
                    + access_list_cost,
                result.evm.reverted,
            );
            let total_gas = total_gas.max(gas_floor);
            eprintln!("GAS: {total_gas}");
            eprintln!("CREATED: {:?}", ext.created_accounts);
        }

        eprintln!("OK: {}", !result.evm.reverted);

        let path = format!("sole.{block_number}.{skip}.log");
        evm_tracer::aux::dump(&path, &traces)?;
        println!("TRACES: {} in {path}", traces.len());

        // Dump STATE:

        use std::collections::{BTreeMap, BTreeSet};
        let mut kv: BTreeMap<Address, BTreeSet<Word>> = BTreeMap::new();
        let mut touched = result.evm.touches
            .iter()
            .filter_map(|touch| match touch {
                solenoid::executor::AccountTouch::GetState(address, key, _, _) => {
                    kv.entry(*address).or_default().insert(*key);
                    Some(*address)
                }
                solenoid::executor::AccountTouch::GetCode(address, _, _) => {
                    Some(*address)
                }
                // touches that modify the state:
                solenoid::executor::AccountTouch::FeePay(address, _, _) => {
                    Some(*address)
                }
                solenoid::executor::AccountTouch::SetState(address, key, _, _, _) => {
                    kv.entry(*address).or_default().insert(*key);
                    Some(*address)
                }
                solenoid::executor::AccountTouch::SetNonce(address, _, _) => {
                    Some(*address)
                }
                solenoid::executor::AccountTouch::SetValue(address, _, _) => {
                    Some(*address)
                }
                solenoid::executor::AccountTouch::Create(address, _, _, _, _) => {
                    Some(*address)
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();

        // Include addresses from access list (REVM includes these in state diff)
        for item in &ext.tx_ctx.access_list {
            touched.insert(item.address);
            for key in &item.storage_keys {
                kv.entry(item.address).or_default().insert(*key);
            }
        }

        let mut ret: BTreeMap<Address, serde_json::Value> = BTreeMap::new();
        for address in touched.into_iter() {
            let account = ext.pull(&address).await?;
            let mut json = serde_json::json!({
                "balance": account.value,
                "nonce": account.nonce.as_u64(),
                "code": String::from("0x") + &hex::encode(account.code.1.into_bytes()),
            });

            let mut state = BTreeMap::new();
            for key in kv.get(&address).cloned().unwrap_or_default() {
                let val = ext.get(&address, &key).await?;
                state.insert(key, val);
            }
            if !state.is_empty() {
                json["state"] = serde_json::to_value(state).unwrap();
            }
            ret.insert(address, json);
        }
        let path = format!("sole.{block_number}.{skip}.state.json");
        evm_tracer::aux::dump(&path, &[ret])?;
        println!("STATE: {path}");

        // Explicitly drop large data structures to free memory immediately
        // Dropping 400k+ traces can take a long time if they're in swap
        drop(traces);
        drop(result);
        
        // Yield to allow the runtime to process the drops
        tokio::task::yield_now().await;
    }
    
    // Explicitly drop ext to close HTTP client connections
    // The reqwest::Client connection pool can keep the Tokio runtime alive
    drop(ext);
    
    Ok(())
}
