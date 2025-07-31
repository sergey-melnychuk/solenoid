use eyre::Context;
use solenoid::{
    common::{
        address::addr,
        word::{Word, word},
    },
    eth,
    ext::Ext,
    solenoid::{Builder, Solenoid},
    tracer::EventTracer,
};

#[tokio::test]
async fn test_tx_0x9b312d7abad8a54cca5735b21304097b700142cea90aeba3740f6a470e734fa6()
-> eyre::Result<()> {
    dotenv::dotenv().ok();
    let _ = tracing_subscriber::fmt::try_init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);
    let mut ext = Ext::latest(eth).await?;

    let from = addr("0xb6b1581b3d267044761156d55717b719ab0565b1");
    ext.acc_mut(&from).balance = Word::from(1_000_000_000_000_000_000u64);
    let to = addr("0x5c2e112783a6854653b4bc7dc22248d3e592559c");
    let method = "";
    let input = hex::decode("b081b4eb")?;

    let sole = Solenoid::new();
    let mut res = sole
        .execute(to, &method, &input)
        .with_sender(from)
        .with_gas(word("0x9a38"))
        .with_value(Word::zero())
        .ready()
        .apply(&mut ext)
        .await
        .context("test-execute")?;
    assert!(!res.evm.reverted);
    // assert_eq!(res.evm.gas.used, word("0x9a28"));
    for e in res.tracer.take() {
        println!("{}", serde_json::to_string(&e)?);
    }

    Ok(())
}

#[tokio::test]
async fn test_tx_0x6d2d94b5bf06ff07cca77f0100233da7d45876cc58595122505ebd124d00d4a1()
-> eyre::Result<()> {
    dotenv::dotenv().ok();
    let _ = tracing_subscriber::fmt::try_init();

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);
    let mut ext = Ext::latest(eth).await?;

    let from = addr("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512");
    ext.acc_mut(&from).balance = Word::from(1_000_000_000_000_000_000u64);
    let to = addr("0x0000000000000068f116a894984e2db1123eb395");
    let method = "";

    let hex = "0000000000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000098b59351f748000000000000000000000000061514b196d5e8e3ff56b71b0631f986285c0e85a0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000bd8451d2d5fb88469a764b05c1e0b623c51061450000000000000000000000000000000000000000000000000000000000006b150000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000688936f30000000000000000000000000000000000000000000000000000000068b0c3f300000000000000000000000000000000000000000000000000000000000000003d958fe2000000000000000000000000000000000000000032fd34e0013cb0db0000007b02230091a7ed01230072f7006a004d60a8d4e71d599b8104250f00000000007b02230091a7ed01230072f7006a004d60a8d4e71d599b8104250f00000000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000024000000000000000000000000000000000000000000000000000000000000002a0000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000c4733b967800000000000000000000000000000a26b00c1f0df003000390027140000faa7190000000000000000000000000000000000000000000000000000000000000040643293cf69bfb8078586f8e8274c2aaf046a7bec4d6611e565f2b37a34a46db156ad1fb1c815ae2155bc0fdccb34e33e777fb45d5d3e8b01c7410cb1b101cc373d958fe2";
    let input = hex::decode(hex)?;

    let sole = Solenoid::new();
    let mut res = sole
        .execute(to, &method, &input)
        .with_sender(from)
        .with_gas(word("0x277e7"))
        .with_value(Word::zero())
        .ready()
        .apply(&mut ext)
        .await
        .context("test-execute")?;
    assert!(res.evm.reverted);
    for e in res.tracer.take() {
        println!("{}", serde_json::to_string(&e)?);
    }

    Ok(())
}
