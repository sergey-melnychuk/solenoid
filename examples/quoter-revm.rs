use evm_tracer::{TxTrace, alloy_eips, alloy_primitives, alloy_provider, eyre, revm};

use alloy_eips::{BlockId, BlockNumberOrTag};
use alloy_primitives::B256;
use alloy_provider::{Provider, ProviderBuilder};
use eyre::Result;

use revm::context::{Context, TxEnv};
use revm::database::{AlloyDB, CacheDB, StateBuilder, WrapDatabaseAsync};
use revm::primitives::{Address, Bytes, TxKind, U256};
use revm::{MainBuilder, MainContext};

use solenoid::common::hash::keccak256;

// RUST_LOG=off cargo run --release --example quoter-revm

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let url = std::env::var("URL")?.parse()?;
    let client = ProviderBuilder::new().connect_http(url);

    let number = 23027350; // 0x15f5e96

    // Get latest block header for context
    let block = client
        .get_block_by_number(BlockNumberOrTag::Number(number))
        .full()
        .await?
        .ok_or_else(|| evm_tracer::eyre::eyre!("Block not found"))?;

    println!("📦 Using block number: {}", block.header.number);

    // Set up state database (exact copy from trace_one)
    let prev_id: BlockId = (block.header.number - 1).into();
    let state_db = WrapDatabaseAsync::new(AlloyDB::new(client.clone(), prev_id))
        .expect("can only fail if tokio runtime is unavailable");
    let cache_db = CacheDB::new(state_db);
    let mut state = StateBuilder::new_with_database(cache_db).build();

    // Set up EVM context with minimal basefee to avoid gas validation issues
    let ctx = Context::mainnet()
        .with_db(&mut state)
        .modify_block_chained(|b| {
            b.number = U256::from(block.header.number);
            b.beneficiary = block.header.beneficiary;
            b.timestamp = U256::from(block.header.timestamp);
            b.difficulty = block.header.difficulty;
            b.gas_limit = block.header.gas_limit;
            b.basefee = 1; // Set basefee to 1 wei to be minimal but valid
        })
        .modify_cfg_chained(|c| {
            c.chain_id = 1;
        });

    // Prepare the call data (same as quoter example)
    let uniswap_v3_quoter: Address = "0x61fFE014bA17989E743c5F6cB21bF9697530B21e".parse()?;
    let from: Address = "0xb18f13b8fde294e0147188a78d5b1328f206f4e2".parse()?;

    // Function selector for quoteExactInputSingle((address,address,uint256,uint24,uint160))
    let method = "quoteExactInputSingle((address,address,uint256,uint24,uint160))";
    let selector = keccak256(method.as_bytes())[..4].to_vec();
    println!("{}", hex::encode(&selector));

    // Prepare function arguments
    let mut call_data = selector;

    // Encode the struct parameter for quoteExactInputSingle
    // struct QuoteExactInputSingleParams {
    //     address tokenIn;     // USDC
    //     address tokenOut;    // WETH
    //     uint256 amountIn;    // 100
    //     uint24 fee;          // 3000
    //     uint160 sqrtPriceLimitX96; // 0
    // }

    let usdc: Address = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".parse()?;
    let weth: Address = "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2".parse()?;

    let amount_in = U256::from(1_000_000_000_000_000_000u128);

    // Encode as ABI-packed struct (32 bytes each)
    call_data.extend_from_slice(&[0u8; 12]); // padding for address
    call_data.extend_from_slice(weth.as_slice()); // WETH
    call_data.extend_from_slice(&[0u8; 12]); // padding for address
    call_data.extend_from_slice(usdc.as_slice()); // USDC
    call_data.extend_from_slice(&amount_in.to_be_bytes::<32>()); // amountIn
    call_data.extend_from_slice(&U256::from(3_000).to_be_bytes::<32>()); // fee
    call_data.extend_from_slice(&U256::ZERO.to_be_bytes::<32>()); // sqrtPriceLimitX96

    for arg in call_data[4..].chunks(32) {
        eprintln!("{}", hex::encode(arg));
    }
    eprintln!("---");

    // Get the correct nonce for the sender
    // let nonce = client.get_transaction_count(from).await?;
    let nonce = 2; // no way to query nonce at given block apparently

    // Create transaction environment (using minimal gas price for quoter call)
    let tx_env = TxEnv::builder()
        .caller(from)
        .gas_limit(1_000_000)
        .value(U256::ZERO)
        .data(Bytes::from(call_data))
        .chain_id(Some(1))
        .nonce(nonce)
        .gas_price(1u128) // Minimal gas price
        .kind(TxKind::Call(uniswap_v3_quoter))
        .build()
        .unwrap();

    // Execute and trace the transaction
    let mut tracer = TxTrace::default();
    tracer.setup(B256::ZERO, from, uniswap_v3_quoter, U256::ZERO, 0);

    use revm::InspectEvm as _;
    let mut evm = ctx.build_mainnet_with_inspector(&mut tracer);
    let result = evm.inspect_tx(tx_env)?;

    // Execute the transaction
    /*
    use revm::ExecuteEvm as _;
    let mut evm = ctx.build_mainnet();
    let result = evm.transact(tx_env)?;
    */

    if let Some(output) = result.result.output() {
        println!("RET:");
        for chunk in output.chunks(32) {
            eprintln!("{}", hex::encode(chunk));
        }

        // Decode QuoterV2 return values:
        // (uint256 amountOut, uint160 sqrtPriceX96After, uint32 initializedTicksCrossed, uint256 gasEstimate)
        if output.len() >= 128 {
            let amount_out = U256::from_be_slice(&output[0..32]);
            let sqrt_price_x96_after = U256::from_be_slice(&output[32..64]); // Note: actually uint160 but stored in 32-byte slot
            let initialized_ticks_crossed = U256::from_be_slice(&output[64..96]); // Note: actually uint32 but stored in 32-byte slot
            let gas_estimate = U256::from_be_slice(&output[96..128]);

            let weth_decimals = 18;
            let usdc_decimals = 6;
            let price_after =
                calculate_price_from_sqrt(sqrt_price_x96_after, usdc_decimals, weth_decimals);

            println!("📊 QuoterV2 Results:");
            println!(
                "  💰 Amount Out: {} WETH for {} USDC",
                format_weth_amount(amount_in),
                format_usdc_amount(amount_out)
            );
            println!("  📊 Price After (WETH/USDC): {}", 1.0 / price_after);
            println!(
                "  🎯 Initialized Ticks Crossed: {}",
                initialized_ticks_crossed
            );
            println!("  ⛽ Gas Estimate: {}", gas_estimate);
        } else {
            println!(
                "⚠️  Unexpected return data length: {} bytes (expected at least 128)",
                output.len()
            );
        }
    }

    println!("✅ Transaction executed successfully!");
    println!("🔄 Reverted: {}", result.result.is_halt());
    println!("⛽ Gas used: {}", result.result.gas_used());

    let path = "quoter-revm.log";
    evm_tracer::aux::dump(path, &tracer.traces)?;
    println!("TRACES: {} in {path}", tracer.traces.len());

    Ok(())
}

fn calculate_price_from_sqrt(
    sqrt_price_x96: U256,
    decimals_token0: u8,
    decimals_token1: u8,
) -> f64 {
    // Convert sqrt_price_x96 to f64
    let sqrt_price_x96_f64 = sqrt_price_x96.to::<u128>() as f64;

    // Calculate the raw price: (sqrtPriceX96 / 2^96)^2
    let q96 = 2_f64.powi(96);
    let sqrt_price = sqrt_price_x96_f64 / q96;
    let raw_price = sqrt_price * sqrt_price;

    // Adjust for decimal differences
    // Price is token1/token0, so we need to adjust for decimal differences
    let decimal_adjustment = 10_f64.powi(decimals_token0 as i32 - decimals_token1 as i32);

    raw_price * decimal_adjustment
}

fn format_weth_amount(amount: U256) -> f64 {
    let weth_decimals = 1e18;
    amount.to::<u128>() as f64 / weth_decimals
}

fn format_usdc_amount(amount: U256) -> f64 {
    let usdc_decimals = 1e6;
    amount.to::<u128>() as f64 / usdc_decimals
}

/*

$ cargo run --example quoter-revm
...
📦 Using block number: 23396227
Function selector: c6a5026a
Call data: c6a5026a000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb480000000000000000000000000000000000000000000000000de0b6b3a76400000000000000000000000000000000000000000000000000000000000000000bb80000000000000000000000000000000000000000000000000000000000000000
✅ Transaction executed successfully!
🔄 Reverted: false
⛽ Gas used: 123286
📤 Return data: 000000000000000000000000000000000000000000000000000000010dc57db400000000000000000000000000000000000039f9f6468911bcf1e46ec4c482d900000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000014bb3
📊 QuoterV2 Results:
  💰 Amount Out: 1 WETH for 4526.013876 USDC
  📊 Price After (WETH/USDC): 4539.597712923113
  🎯 Initialized Ticks Crossed: 1
  ⛽ Gas Estimate: 84915

*/
