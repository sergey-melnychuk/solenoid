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

        // TX:5
        let acc = addr("0x8778f133d11e81a05f5210b317fb56115b95c7bc");
        ext.pull(&acc).await?;
        ext.acc_mut(&acc).value = word("0x7af6c7f27291f2ff0");

        // TX:6 - OK

        // TX:7 - precompile 0x1 (ecrecover) missing
        let acc = addr("0xbb318a1ab8e46dfd93b3b0bca3d0ebf7d00187b9");
        ext.pull(&acc).await?;
        ext.acc_mut(&acc).value = word("0x");

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

        if tx.to.is_some() {
            let call_cost = 21000i64;
            let data_cost = {
                let total_calldata_len = tx.input.as_ref().len();
                let nonzero_bytes_count = tx.input.as_ref().iter().filter(|byte| *byte != &0).count();
                nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
            } as i64;
            let exec_cost = result.evm.gas.finalized();
            let total_gas = call_cost + data_cost + exec_cost;
            // eprintln!("DEBUG: call_cost={call_cost}, data_cost={data_cost}, exec_cost={exec_cost}");
            eprintln!("GAS: {total_gas}");
        } else {
            /*
            (https://www.evm.codes/?fork=cancun#f0)

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
                let nonzero_bytes_count = tx.input.as_ref().iter().filter(|byte| *byte != &0).count();
                nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
            } as i64;
            let create_cost = 32000i64;
            let init_code_cost = 2 * tx.input.as_ref().len().div_ceil(32) as i64;

            let exec_cost = result.evm.gas.finalized();
            let deployed_code_cost = 200 * result.ret.len() as i64;

            let total_gas = call_cost + data_cost + create_cost + init_code_cost + exec_cost + deployed_code_cost;
            // eprintln!("DEBUG: call_cost={call_cost}, data_cost={data_cost}, exec_cost={exec_cost}");
            // eprintln!("DEBUG: create_cost={create_cost}, init_code_cost={init_code_cost}, deployed_code_cost={deployed_code_cost}");
            eprintln!("GAS: {total_gas} [created={}]", result.created.expect("contract should have been created"));
        }

        eprintln!("OK: {}", !result.evm.reverted);

        let path = format!("sole.{block_number}.{skip}.log");
        evm_tracer::aux::dump(&path, &traces)?;
        println!("TRACES: {} in {path}", traces.len());
    }
    Ok(())
}
