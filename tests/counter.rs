use primitive_types::U256;
use solenoid::{
    common::address::Address,
    common::call::Call,
    decoder::{Bytecode, Decoder},
    eth::EthClient,
    executor::{Evm, Executor},
    ext::Ext,
    tracer::NoopTracer,
};

static CODE: &str = include_str!("../etc/counter/Counter.bin-runtime");

fn code() -> eyre::Result<Bytecode> {
    let code = hex::decode(CODE.trim_start_matches("0x"))?;
    Ok(Decoder::decode(code)?)
}

async fn call(
    calldata: &str,
    to: Address,
    overrides: Vec<(Address, U256, U256)>,
) -> eyre::Result<(NoopTracer, Evm, Vec<u8>)> {
    let value = U256::zero();
    let from = Address::try_from("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266")?;
    let call = Call {
        calldata: hex::decode(calldata.trim_start_matches("0x"))?,
        value,
        from,
        to,
        gas: U256::zero(),
    };

    // TODO: use mock http server for hermetic tests
    let url = std::env::var("URL")?;
    let eth = EthClient::new(&url);
    let (_, block_hash) = eth.get_latest_block().await?;

    let mut ext = Ext::new(block_hash, eth);
    for (to, key, val) in overrides {
        ext.put(&to, key, val).await?;
    }

    let executor = Executor::<NoopTracer>::new();
    Ok(executor.execute(&code()?, &call, &mut ext).await?)
}

#[tokio::test]
async fn test_deploy() -> eyre::Result<()> {
    let code = include_str!("../etc/counter/Counter.bin");
    let code = hex::decode(code)?;
    let code = Decoder::decode(code)?;
    let to = Address::zero();

    // TODO: extract EthClient trait and provide mock impl here?
    let url = std::env::var("URL")?;
    let eth = EthClient::new(&url);
    let (_, block_hash) = eth.get_latest_block().await?;

    let mut ext = Ext::new(block_hash, eth);
    let executor = Executor::<NoopTracer>::new();

    let value = U256::zero();
    let from = Address::try_from("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266")?;
    let call = Call {
        calldata: vec![],
        value,
        from,
        to,
        gas: U256::zero(),
    };
    let (_, evm, ret) = executor.execute(&code, &call, &mut ext).await?;

    assert!(!evm.reverted);
    let exp = hex::decode(CODE.trim_start_matches("0x"))?;
    assert_eq!(ret, exp);
    Ok(())
}

#[tokio::test]
async fn test_get() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = Address::try_from("e7f1725E7734CE288F8367e1Bb143E90bb3F0512")?;
    let (_, evm, ret) = call("0x6d4ce63c", to, vec![]).await?;
    assert!(!evm.reverted);
    assert_eq!(ret, vec![0u8; 32]);
    assert_eq!(evm.state, vec![(to, U256::zero(), U256::zero(), None),]);
    Ok(())
}

#[tokio::test]
async fn test_get_with_override() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = Address::try_from("e7f1725E7734CE288F8367e1Bb143E90bb3F0512")?;
    let (_, evm, ret) = call(
        "0x6d4ce63c",
        to,
        vec![(
            Address::try_from("e7f1725E7734CE288F8367e1Bb143E90bb3F0512")?,
            U256::zero(),
            U256::one(),
        )],
    )
    .await?;
    assert!(!evm.reverted);
    assert_eq!(
        ret,
        hex::decode("0000000000000000000000000000000000000000000000000000000000000001")?
    );
    assert_eq!(evm.state, vec![(to, U256::zero(), U256::one(), None),]);
    Ok(())
}

#[tokio::test]
async fn test_dec() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = Address::try_from("e7f1725E7734CE288F8367e1Bb143E90bb3F0512")?;
    let (_, evm, ret) = call("0xb3bcfa82", to, vec![]).await?;
    assert!(evm.reverted);
    assert_eq!(
        ret,
        hex::decode("4e487b710000000000000000000000000000000000000000000000000000000000000011")?
    );
    assert_eq!(evm.state, vec![(to, U256::zero(), U256::zero(), None),]);
    Ok(())
}

#[tokio::test]
async fn test_dec_with_override() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = Address::try_from("e7f1725E7734CE288F8367e1Bb143E90bb3F0512")?;
    let (_, evm, ret) = call(
        "0xb3bcfa82",
        to,
        vec![(
            Address::try_from("e7f1725E7734CE288F8367e1Bb143E90bb3F0512")?,
            U256::zero(),
            U256::one(),
        )],
    )
    .await?;
    assert!(!evm.reverted);
    assert_eq!(ret, vec![0u8; 0]);
    assert_eq!(
        evm.state,
        vec![
            (to, U256::zero(), U256::one(), None),
            (to, U256::zero(), U256::one(), Some(U256::zero())),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn test_inc() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = Address::try_from("e7f1725E7734CE288F8367e1Bb143E90bb3F0512")?;
    let (_, evm, ret) = call("0x371303c0", to, vec![]).await?;
    assert!(!evm.reverted);
    assert_eq!(ret, vec![0u8; 0]);
    assert_eq!(
        evm.state,
        vec![
            (to, U256::zero(), U256::zero(), None),
            (to, U256::zero(), U256::zero(), Some(U256::one())),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn test_set() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let to = Address::try_from("e7f1725E7734CE288F8367e1Bb143E90bb3F0512")?;
    let (_, evm, ret) = call(
        "0x60fe47b10000000000000000000000000000000000000000000000000000000000000042",
        to,
        vec![],
    )
    .await?;
    assert!(!evm.reverted);
    assert_eq!(ret, vec![0u8; 0]);
    assert_eq!(
        evm.state,
        vec![(
            to,
            U256::zero(),
            U256::zero(),
            Some(U256::from_str_radix("42", 16)?)
        )]
    );
    Ok(())
}
