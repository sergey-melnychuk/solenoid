use solenoid::{
    common::{
        address::{Address, addr},
        call::Call,
        hash::keccak256,
        word::{Word, word},
    },
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
    let code = Decoder::decode(code);
    let to = Address::zero();

    // TODO: extract EthClient trait and provide mock impl here?
    let url = std::env::var("URL")?;
    let eth = EthClient::new(&url);
    let mut ext = Ext::at_latest(eth).await?;

    let executor = Executor::<NoopTracer>::new();

    let value = Word::zero();
    let from = addr("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512");
    let call = Call {
        data: vec![],
        value,
        from,
        to,
        gas: Word::from(1_000_000),
    };

    ext.acc_mut(&from).nonce = Word::zero();
    let created1 = from.of_smart_contract(Word::zero());
    let created2 = created1.of_smart_contract(Word::zero());

    let mut evm = Evm::default();
    let (_, ret) = executor.execute(&code, &call, &mut evm, &mut ext).await?;

    assert!(!evm.reverted);
    assert_eq!(ret, hex::decode(CALL.trim_start_matches("0x"))?);

    let code = hex::decode(CELL.trim_start_matches("0x"))?;
    assert_eq!(
        evm.account,
        vec![
            AccountTouch::Code(created2, Word::from_bytes(&keccak256(&code)), code),
            AccountTouch::Nonce(from, 0, 1)
        ]
    );
    pretty_assertions::assert_eq!(
        evm.state,
        vec![
            StateTouch(created1, Word::zero(), Word::zero(), None, Word::zero()),
            StateTouch(
                created1,
                Word::zero(),
                Word::zero(),
                Some((&from).into()),
                Word::zero()
            ),
            StateTouch(
                created2,
                Word::zero(),
                Word::zero(),
                Some(word("0x42")),
                Word::zero()
            ),
            StateTouch(created1, Word::one(), Word::zero(), None, Word::zero()),
            StateTouch(
                created1,
                Word::one(),
                Word::zero(),
                Some((&created2).into()),
                Word::zero()
            ),
        ]
    );
    Ok(())
}
