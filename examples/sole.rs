use eyre::{Context, eyre};
use solenoid::{
    common::{
        address::Address,
        hash,
        word::Word,
    },
    eth,
    ext::Ext,
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

        // eprintln!("TX: {tx:#?}");
        // eprintln!("TX hash={:#064x} index={}", tx.hash, tx.index.as_usize());
        // eprintln!("GAS PRICE: {}", tx.gas_price.as_u64());
        // eprintln!("GAS LIMIT: {}", tx.gas.as_u64());

        ext.reset(tx.gas_price);
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
            eprintln!(
                "RET: len={} hash={}",
                result.ret.len(),
                Word::from_bytes(&hash::keccak256(&result.ret))
            );
        }

        if tx.to.is_some() {
            let call_cost = 21000i64;
            let data_cost = {
                let total_calldata_len = tx.input.as_ref().len();
                let nonzero_bytes_count =
                    tx.input.as_ref().iter().filter(|byte| *byte != &0).count();
                nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
            } as i64;
            let total_gas = result.evm.gas.finalized(call_cost + data_cost);
            // eprintln!("DEBUG: call_cost={call_cost}, data_cost={data_cost}");
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
                call_cost + data_cost + create_cost + init_code_cost + deployed_code_cost,
            );
            // eprintln!("DEBUG: call_cost={call_cost}, data_cost={data_cost}");
            // eprintln!("DEBUG: create_cost={create_cost}, init_code_cost={init_code_cost}, deployed_code_cost={deployed_code_cost}");
            eprintln!(
                "GAS: {total_gas} [created={}]",
                result.created.expect("contract should have been created")
            );
        }

        eprintln!("OK: {}", !result.evm.reverted);

        let path = format!("sole.{block_number}.{skip}.log");
        evm_tracer::aux::dump(&path, &traces)?;
        println!("TRACES: {} in {path}", traces.len());
    }
    Ok(())
}
