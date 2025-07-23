use solenoid::{
    common::{Word, addr, address::Address, call::Call, hash::keccak256, word},
    decoder::Decoder,
    eth::EthClient,
    executor::{AccountTouch, Evm, Executor, StateTouch},
    ext::Ext,
    tracer::NoopTracer,
};

static CALL: &str = include_str!("../etc/call/Call.bin-runtime");

static CELL: &str = include_str!("../etc/call/Cell.bin-runtime");

#[tokio::test]
async fn test_deploy() -> eyre::Result<()> {
    dotenv::dotenv()?;
    let code = include_str!("../etc/call/Call.bin");
    let code = hex::decode(code)?;
    let code = Decoder::decode(code)?;
    let to = Address::zero();

    // TODO: extract EthClient trait and provide mock impl here?
    let url = std::env::var("URL")?;
    let eth = EthClient::new(&url);
    let mut ext = Ext::latest(eth).await?;

    let executor = Executor::<NoopTracer>::new();

    let value = Word::zero();
    let from = addr("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512");
    let call = Call {
        data: vec![],
        value,
        from,
        to,
        gas: Word::from(110_000),
    };
    let mut evm = Evm::default();
    let (_, ret) = executor.execute(&code, &call, &mut evm, &mut ext).await?;

    assert!(!evm.reverted);
    assert_eq!(ret, hex::decode(CALL.trim_start_matches("0x"))?);

    let code = hex::decode(CELL.trim_start_matches("0x"))?;
    assert_eq!(
        evm.account,
        vec![
            AccountTouch::Code(
                addr("0xc80a141ce8a5b73371043cba5cee40437975bb37"),
                Word::from_big_endian(&keccak256(&code)),
                code
            ),
            AccountTouch::Nonce(addr("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512"), 0, 1)
        ]
    );
    assert_eq!(
        evm.state,
        vec![
            StateTouch(
                Address::zero(),
                Word::zero(),
                Word::zero(),
                None,
                Word::zero()
            ),
            StateTouch(
                Address::zero(),
                Word::zero(),
                Word::zero(),
                Some(word("e7f1725e7734ce288f8367e1bb143e90bb3f0512")),
                Word::zero()
            ),
            StateTouch(
                addr("c80a141ce8a5b73371043cba5cee40437975bb37"),
                Word::zero(),
                word("e7f1725e7734ce288f8367e1bb143e90bb3f0512"),
                Some(Word::from(66)),
                Word::zero()
            ),
            StateTouch(
                Address::zero(),
                Word::one(),
                Word::zero(),
                None,
                Word::zero()
            ),
            StateTouch(
                Address::zero(),
                Word::one(),
                Word::zero(),
                Some(word("c80a141ce8a5b73371043cba5cee40437975bb37")),
                Word::zero()
            ),
        ]
    );
    Ok(())
}
