use eyre::Context;
use solenoid::{
    common::{Word, addr},
    eth,
    ext::Ext,
    solenoid::{Builder, Solenoid},
};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);
    let mut ext = Ext::latest(eth).await?;

    let code = include_str!("../etc/call/Call.bin");
    let code = hex::decode(code.trim_start_matches("0x"))?;

    let from = addr("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512");
    ext.acc_mut(&from).balance = Word::from(100_000_000_000_000_000u64);

    let sole = Solenoid::new();
    let res = sole
        .create(code)
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .with_value(Word::zero())
        .ready()
        .apply(&mut ext)
        .await
        .context("create")?;

    let address = res
        .created
        .ok_or_else(|| eyre::eyre!("No address returned"))?;
    println!("Contract deployed at: {address}");

    let res = sole
        .execute(address, "get_owner()", &[])
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    println!("Owner: {}", hex::encode(res.ret));

    let res = sole
        .execute(address, "get()", &[])
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    println!("get(): {}", hex::encode(res.ret));

    let res = sole
        .transfer(address, Word::from(42))
        .with_sender(from)
        .with_gas(Word::from(21000))
        .ready()
        .apply(&mut ext)
        .await
        .context("transfer")?;
    println!("TX OK: {}", !res.evm.reverted);

    Ok(())
}
