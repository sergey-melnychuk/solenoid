use eyre::Context;
use solenoid::{
    common::{address::addr, word::Word},
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
    /* Uncomment lines below to avoid JSON-RPC requests */
    // ext.data_mut(&addr("0xc26297fdd7b51a5c8c4ffe76f06af56680e2b552"))
    //     .insert(Word::zero(), Word::zero()); // Call.owner
    // ext.data_mut(&addr("0xc80a141ce8a5b73371043cba5cee40437975bb37"))
    //     .insert(Word::zero(), Word::zero()); // Call.target
    // ext.data_mut(&addr("0xc80a141ce8a5b73371043cba5cee40437975bb37"))
    //     .insert(Word::one(), Word::zero()); // Cell.value

    let code = include_str!("../etc/call/Call.bin");
    let code = hex::decode(code.trim_start_matches("0x"))?;

    let from = addr("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512");
    ext.acc_mut(&from).value = Word::from(100_000_000_000_000_000u64);

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
    println!("Call address: {address}");

    let res = sole
        .execute(address, "get_owner()", &[])
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    println!("Call.get_owner(): {}", hex::encode(res.ret));

    let res = sole
        .execute(address, "get()", &[])
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    println!("Call.get(): {}", hex::encode(res.ret));

    let res = sole
        .execute(address, "set(uint256)", &Word::one().into_bytes())
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    println!("Call.set(0x01): {}", hex::encode(res.ret));

    let res = sole
        .execute(address, "get()", &[])
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    println!("Call.get(): {}", hex::encode(res.ret));

    let cell = address.of_smart_contract(Word::zero());
    println!("Cell address: {cell}");

    let res = sole
        .execute(cell, "get()", &[])
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    println!("Cell.get(): {}", hex::encode(res.ret));

    let res = sole
        .execute(cell, "set(uint256)", &Word::from(0xff).into_bytes())
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    println!("Cell.set(0xff): {}", hex::encode(res.ret));

    let res = sole
        .execute(cell, "get()", &[])
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    println!("Cell.get(): {}", hex::encode(res.ret));

    let res = sole
        .execute(address, "get()", &[])
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    println!("Call.get(): {}", hex::encode(res.ret));

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
