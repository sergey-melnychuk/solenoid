use eyre::Context;
use solenoid::{
    common::{address::{addr, Address}, hash::keccak256, word::{decode_error_string, Word}},
    eth,
    ext::Ext,
    solenoid::{Builder, Solenoid},
};

const UNISWAP_V3_QUOTER: Address = addr("0x61fFE014bA17989E743c5F6cB21bF9697530B21e"); // Quoter V2

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);
    let mut ext = Ext::at_latest(eth).await?;

    let from = addr("0xb18f13b8fde294e0147188a78d5b1328f206f4e2");

    // Uniswap V3 QuoterV2: https://etherscan.io/address/0x61fFE014bA17989E743c5F6cB21bF9697530B21e
    // SOURCE: https://github.com/Uniswap/v3-periphery/blob/main/contracts/interfaces/IQuoterV2.sol

    let method = "quoteExactInputSingle((address,address,uint256,uint24,uint160))";
    eprintln!("{}", hex::encode(&keccak256(method.as_bytes())[..4]));

    let mut args = Vec::new();
    args.extend_from_slice(&addr("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48").as_word().into_bytes()); // USDC address
    args.extend_from_slice(&addr("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2").as_word().into_bytes()); // WETH address
    args.extend_from_slice(&Word::from(3_000).into_bytes()); // fee (3000 basis points = 0.3%)
    args.extend_from_slice(&Word::from(100).into_bytes()); // amountIn (much smaller test)
    args.extend_from_slice(&Word::zero().into_bytes()); // sqrtPriceLimitX96 (0 for no limit)

    for arg in args.chunks(32) {
        eprintln!("{}", hex::encode(arg));
    }
    eprintln!("---");

    let sole = Solenoid::new();
    let res = sole
        .execute(UNISWAP_V3_QUOTER, method, &args)
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    println!("EVM: OK={}", !res.evm.reverted);

    if let Some(error) = decode_error_string(&res.ret) {
        println!("ERR: '{error}'");
    } else {
        println!("RET: {}", hex::encode(&res.ret));
    }

    Ok(())
}
