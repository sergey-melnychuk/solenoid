use eyre::Context;
use solenoid::{
    common::{address::{addr, Address}, hash::keccak256, word::{decode_error_string, Word}},
    eth,
    ext::Ext,
    solenoid::{Builder, Solenoid},
};

const UNISWAP_V3_QUOTER: Address = addr("0xb27308f9F90D607463bb33eA1BeBb41C27CE5AB6"); // Quoter V1

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);
    let mut ext = Ext::at_latest(eth).await?;

    let from = addr("0xb18f13b8fde294e0147188a78d5b1328f206f4e2");

    // https://github.com/Uniswap/v3-periphery/blob/main/contracts/interfaces/IQuoter.sol
    /*
    function quoteExactInputSingle(
        address tokenIn,
        address tokenOut,
        uint24 fee,
        uint256 amountIn,
        uint160 sqrtPriceLimitX96
    ) external returns (uint256 amountOut);
    */

    /*
    Uniswap V3 Quoter V1 : https://etherscan.io/address/0xb27308f9f90d607463bb33ea1bebb41c27ce5ab6
    TX: https://etherscan.io/tx/0x73a2ab07d2a9abdb9e0520358637e34e0053e7b7e27da3fe9a4ce4560139cafb
    
    quoteExactInputSingle(address tokenIn, address tokenOut, uint24 fee, uint256 amountIn, uint160 sqrtPriceLimitX96)

    f7729d43
    000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48
    000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2
    0000000000000000000000000000000000000000000000000000000000002710
    00000000000000000000000000000000000000000000000000000000000f4240
    0000000000000000000000000000000000000000000000000000000000000000

    ---

    f7729d43
    000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48
    000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2
    0000000000000000000000000000000000000000000000000000000000002710
    00000000000000000000000000000000000000000000000000000000000f4240
    0000000000000000000000000000000000000000000000000000000000000000
     */

    let method = "quoteExactInputSingle(address,address,uint24,uint256,uint160)";
    eprintln!("{}", hex::encode(&keccak256(method.as_bytes())[..4]));

    let mut args = Vec::new();
    args.extend_from_slice(&Word::from_bytes(&addr("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48").0).into_bytes()); // USDC address
    args.extend_from_slice(&Word::from_bytes(&addr("c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2").0).into_bytes()); // WETH address
    args.extend_from_slice(&Word::from(3_000u64).into_bytes()); // fee (3000 basis points = 0.3%)
    args.extend_from_slice(&Word::from(100u64).into_bytes()); // amountIn (much smaller test)
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

