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

    let number = 23027350; // 0x15f5e96
    let header = eth.get_block_header(Word::from(number)).await?;
    let mut ext = Ext::at_number(Word::from(number - 1), eth).await?;

    let from = addr("0xb18f13b8fde294e0147188a78d5b1328f206f4e2");

    // Uniswap V3 QuoterV2: https://etherscan.io/address/0x61fFE014bA17989E743c5F6cB21bF9697530B21e
    // SOURCE: https://github.com/Uniswap/v3-periphery/blob/main/contracts/interfaces/IQuoterV2.sol

    let method = "quoteExactInputSingle((address,address,uint256,uint24,uint160))";
    eprintln!("{}", hex::encode(&keccak256(method.as_bytes())[..4]));

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
    args.extend_from_slice(&Word::from(1_000_000_000_000_000_000u128).into_bytes()); // amountIn
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
    println!("EVM: OK={}", !result.evm.reverted);

    if let Some(error) = decode_error_string(&result.ret) {
        println!("ERR: '{error}'");
    } else {
        println!("RET: {}", hex::encode(&result.ret));
    }

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
