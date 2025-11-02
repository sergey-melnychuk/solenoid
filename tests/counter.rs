use solenoid::{
    common::{
        address::{Address, addr},
        call::Call,
        word::Word,
    },
    decoder::{Bytecode, Decoder},
    eth::EthClient,
    executor::{AccountTouch, Evm, Executor},
    ext::Ext,
    tracer::NoopTracer,
};

static CODE: &str = include_str!("../etc/counter/Counter.bin-runtime");

fn code() -> eyre::Result<Bytecode> {
    let code = hex::decode(CODE.trim_start_matches("0x"))?;
    Ok(Decoder::decode(code))
}

async fn call(
    calldata: &str,
    to: Address,
    overrides: Vec<(Address, Word, Word)>,
) -> eyre::Result<(NoopTracer, Vec<u8>, Evm)> {
    let value = Word::zero();
    let from = addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266");
    let call = Call {
        data: hex::decode(calldata.trim_start_matches("0x"))?,
        value,
        from,
        to,
        gas: Word::from(100500),
    };

    // TODO: use mock http server for hermetic tests
    let url = std::env::var("URL")?;
    let eth = EthClient::new(&url);
    let mut ext = Ext::at_number(Word::from(23505042), eth).await?;

    for (to, key, val) in overrides {
        ext.put(&to, key, val).await?;
    }

    let executor = Executor::<NoopTracer>::new();
    let mut evm = Evm::default();
    let (tracer, ret) = executor
        .execute(&code()?, &call, &mut evm, &mut ext)
        .await?;
    Ok((tracer, ret, evm))
}

#[tokio::test]
async fn test_deploy() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let code = include_str!("../etc/counter/Counter.bin");
    let code = hex::decode(code)?;
    let code = Decoder::decode(code);
    let to = Address::zero();

    // TODO: extract EthClient trait and provide mock impl here?
    let url = std::env::var("URL")?;
    let eth = EthClient::new(&url);
    let mut ext = Ext::at_number(Word::from(23505042), eth).await?;

    let executor = Executor::<NoopTracer>::new();

    let value = Word::zero();
    let from = addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512");
    let call = Call {
        data: vec![],
        value,
        from,
        to,
        gas: Word::from(100500),
    };
    let mut evm = Evm::default();
    let (_, ret) = executor.execute(&code, &call, &mut evm, &mut ext).await?;

    assert!(!evm.reverted);
    let exp = hex::decode(CODE.trim_start_matches("0x"))?;
    assert_eq!(ret, exp);
    Ok(())
}

#[tokio::test]
async fn test_get() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512");
    let (_, ret, evm) = call("0x6d4ce63c", to, vec![]).await?;
    assert!(!evm.reverted);
    assert_eq!(ret, vec![0u8; 32]);
    assert_eq!(
        evm.touches,
        vec![
            AccountTouch::WarmUp(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266")),
            AccountTouch::WarmUp(addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512")),
            AccountTouch::SetNonce(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"), 2887, 2888),
            AccountTouch::GetState(to, Word::zero(), Word::zero(), false),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn test_get_with_override() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512");
    let (_, ret, evm) = call(
        "0x6d4ce63c",
        to,
        vec![(
            addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512"),
            Word::zero(),
            Word::one(),
        )],
    )
    .await?;
    assert!(!evm.reverted);
    assert_eq!(
        ret,
        hex::decode("0000000000000000000000000000000000000000000000000000000000000001")?
    );
    assert_eq!(
        evm.touches,
        vec![
            AccountTouch::WarmUp(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266")),
            AccountTouch::WarmUp(addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512")),
            AccountTouch::SetNonce(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"), 2887, 2888),
            AccountTouch::GetState(to, Word::zero(), Word::one(), false),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn test_dec() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512");
    let (_, ret, evm) = call("0xb3bcfa82", to, vec![]).await?;
    assert!(evm.reverted);
    assert_eq!(
        ret,
        hex::decode("4e487b710000000000000000000000000000000000000000000000000000000000000011")?
    );
    assert_eq!(
        evm.touches,
        vec![
            AccountTouch::WarmUp(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266")),
            AccountTouch::WarmUp(addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512")),
            AccountTouch::SetNonce(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"), 2887, 2888),
            AccountTouch::GetState(to, Word::zero(), Word::zero(), false),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn test_dec_with_override() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512");
    let (_, ret, evm) = call(
        "0xb3bcfa82",
        to,
        vec![(
            addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512"),
            Word::zero(),
            Word::one(),
        )],
    )
    .await?;
    assert!(!evm.reverted);
    assert_eq!(ret, vec![0u8; 0]);
    assert_eq!(
        evm.touches,
        vec![
            AccountTouch::WarmUp(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266")),
            AccountTouch::WarmUp(addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512")),
            AccountTouch::SetNonce(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"), 2887, 2888),
            AccountTouch::GetState(to, Word::zero(), Word::one(), false),
            AccountTouch::SetState(to, Word::zero(), Word::one(), Word::zero(), true),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn test_inc() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512");
    let (_, ret, evm) = call("0x371303c0", to, vec![]).await?;
    assert!(!evm.reverted);
    assert_eq!(ret, vec![0u8; 0]);
    assert_eq!(
        evm.touches,
        vec![
            AccountTouch::WarmUp(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266")),
            AccountTouch::WarmUp(addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512")),
            AccountTouch::SetNonce(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"), 2887, 2888),
            AccountTouch::GetState(to, Word::zero(), Word::zero(), false),
            AccountTouch::SetState(to, Word::zero(), Word::zero(), Word::one(), true),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn test_set() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512");

    let val = Word::from_hex("42")?;
    let (_, ret, evm) = call(&format!("0x60fe47b1{val:064x}"), to, vec![]).await?;
    assert!(!evm.reverted);
    assert_eq!(ret, vec![0u8; 0]);

    assert_eq!(
        evm.touches,
        vec![
            AccountTouch::WarmUp(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266")),
            AccountTouch::WarmUp(addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512")),
            AccountTouch::SetNonce(addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266"), 2887, 2888),
            AccountTouch::SetState(to, Word::zero(), Word::zero(), val, false),
        ]
    );
    assert_eq!(evm.gas.used, 22309);
    Ok(())
}
