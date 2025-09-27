use eyre::Context;
use solenoid::{
    common::{
        address::{Address, addr},
        hash::keccak256,
        word::{Word, decode_error_string},
    },
    eth,
    ext::Ext,
    solenoid::{Builder, Solenoid},
    tracer::EventTracer,
};

const UNISWAP_V3_QUOTER: Address = addr("0x61fFE014bA17989E743c5F6cB21bF9697530B21e"); // Quoter V2

// RUST_LOG=off cargo run --release --example quoter-sole

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);

    let number = 23448157;
    let header = eth.get_block_header(Word::from(number)).await?;
    let mut ext = Ext::at_number(Word::from(number - 1), eth).await?;

    println!("📦 Using block number: {}", header.number.as_u64());

    let from = addr("0xb18f13b8fde294e0147188a78d5b1328f206f4e2");

    // Uniswap V3 QuoterV2: https://etherscan.io/address/0x61fFE014bA17989E743c5F6cB21bF9697530B21e
    // SOURCE: https://github.com/Uniswap/v3-periphery/blob/main/contracts/interfaces/IQuoterV2.sol

    let method = "quoteExactInputSingle((address,address,uint256,uint24,uint160))";
    eprintln!("{}", hex::encode(&keccak256(method.as_bytes())[..4]));

    let amount_in = Word::from(1_000_000_000_000_000_000u128);

    let mut args = Vec::new();
    args.extend_from_slice(
        &addr("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2")
            .as_word()
            .into_bytes(),
    ); // WETH address
    args.extend_from_slice(
        &addr("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48")
            .as_word()
            .into_bytes(),
    ); // USDC address
    args.extend_from_slice(&amount_in.into_bytes());
    args.extend_from_slice(&Word::from(3_000).into_bytes()); // fee (3000 basis points = 0.3%)
    args.extend_from_slice(&Word::zero().into_bytes()); // sqrtPriceLimitX96 (0 for no limit)

    for arg in args.chunks(32) {
        eprintln!("{}", hex::encode(arg));
    }
    eprintln!("---");

    let sole = Solenoid::new();
    let mut result = sole
        .execute(UNISWAP_V3_QUOTER, method, &args)
        .with_header(header)
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;

    if let Some(error) = decode_error_string(&result.ret) {
        println!("ERR: '{error}'");
    } else {
        println!("RET:");
        for chunk in result.ret.chunks(32) {
            eprintln!("{}", hex::encode(chunk));
        }
        decode_quoter_output(&result.ret, amount_in);
    }

    let call_cost = 21000i64;
    let data_cost = {
        let total_calldata_len = args.len();
        let nonzero_bytes_count = args.iter().filter(|byte| *byte != &0).count();
        nonzero_bytes_count * 16 + (total_calldata_len - nonzero_bytes_count) * 4
    };
    let total_tx_cost = call_cost + data_cost as i64;
    let final_gas_with_tx_cost = result.evm.gas.finalized() + total_tx_cost;
    eprintln!("DEBUG: tx_cost={}, execution_gas={}, refunded_gas={}, final_total={}",
          total_tx_cost, result.evm.gas.used, result.evm.gas.refund, final_gas_with_tx_cost);

    println!("✅ Transaction executed successfully!");
    println!("🔄 Reverted: {}", result.evm.reverted);
    println!("⛽ Gas used: {}", final_gas_with_tx_cost);
    // TODO: FIXME: 4064 gas still missing ¯\_(ツ)_/¯ (revm=123290 sole=119226)

    let path = "quoter-sole.log";
    let traces = result
        .tracer
        .take()
        .into_iter()
        .filter_map(|event| evm_tracer::OpcodeTrace::try_from(event).ok())
        .collect::<Vec<_>>();
    evm_tracer::aux::dump(path, &traces)?;
    println!("TRACES: {} in {path}", traces.len());

    Ok(())
}

fn decode_quoter_output(output: &[u8], amount_in: Word) {
    // Decode QuoterV2 return values:
    // (uint256 amountOut, uint160 sqrtPriceX96After, uint32 initializedTicksCrossed, uint256 gasEstimate)
    if output.len() >= 128 {
        let amount_out = Word::from_bytes(&output[0..32]);
        let sqrt_price_x96_after = Word::from_bytes(&output[32..64]); // Note: actually uint160 but stored in 32-byte slot
        let initialized_ticks_crossed = Word::from_bytes(&output[64..96]); // Note: actually uint32 but stored in 32-byte slot
        let gas_estimate = Word::from_bytes(&output[96..128]);

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

fn calculate_price_from_sqrt(
    sqrt_price_x96: Word,
    decimals_token0: u8,
    decimals_token1: u8,
) -> f64 {
    // Convert sqrt_price_x96 to f64
    let sqrt_price_x96_f64 = sqrt_price_x96.as_u128() as f64;

    // Calculate the raw price: (sqrtPriceX96 / 2^96)^2
    let q96 = 2_f64.powi(96);
    let sqrt_price = sqrt_price_x96_f64 / q96;
    let raw_price = sqrt_price * sqrt_price;

    // Adjust for decimal differences
    // Price is token1/token0, so we need to adjust for decimal differences
    let decimal_adjustment = 10_f64.powi(decimals_token0 as i32 - decimals_token1 as i32);

    raw_price * decimal_adjustment
}

fn format_weth_amount(amount: Word) -> f64 {
    let weth_decimals = 1e18;
    amount.as_u128() as f64 / weth_decimals
}

fn format_usdc_amount(amount: Word) -> f64 {
    let usdc_decimals = 1e6;
    amount.as_u128() as f64 / usdc_decimals
}

/*

$ cargo run --example quoter-sole
...
📦 Using block number: 23027350
c6a5026a
000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2
000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48
0000000000000000000000000000000000000000000000000de0b6b3a7640000
0000000000000000000000000000000000000000000000000000000000000bb8
0000000000000000000000000000000000000000000000000000000000000000
---
RET:
00000000000000000000000000000000000000000000000000000000df3da755
0000000000000000000000000000000000003fbbeb272536a77eac6dce8bfc61
0000000000000000000000000000000000000000000000000000000000000001
0000000000000000000000000000000000000000000000000000000000016982
📊 QuoterV2 Results:
  💰 Amount Out: 1 WETH for 3745.359701 USDC
  📊 Price After (WETH/USDC): 3756.4441989793545
  🎯 Initialized Ticks Crossed: 1
  ⛽ Gas Estimate: 16982 [REVM: 92546]
✅ Transaction executed successfully!
🔄 Reverted: false
⛽ Gas used: 104637 [REVM: 130917]
TRACES: 9221 in quoter-sole.log

*/
