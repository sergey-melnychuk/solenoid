use solenoid::{
    common::{
        address::{Address, addr},
        call::Call,
        hash::keccak256,
        word::{Word, word},
    },
    decoder::Decoder,
    eth::EthClient,
    executor::{AccountTouch, Evm, Executor},
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
    let mut ext = Ext::at_number(Word::from(23505042), eth).await?;

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

    ext.pull(&from).await?;
    ext.account_mut(&from).nonce = Word::zero();
    let created1 = from.create(Word::zero());
    let created2 = created1.create(Word::one());

    let mut evm = Evm::default();
    let (_, ret) = executor.execute(&code, &call, &mut evm, &mut ext).await?;

    assert!(!evm.reverted);
    assert_eq!(hex::encode(ret), CALL.trim_start_matches("0x"));

    let code = hex::decode(CELL.trim_start_matches("0x"))?;
    let hash = keccak256(&code);
    pretty_assertions::assert_eq!(
        evm.touches,
        vec![
            AccountTouch::WarmUp(from),
            AccountTouch::SetNonce(from, 0, 1),
            AccountTouch::GetState(created1, word("0x0"), word("0x0"), false),
            AccountTouch::SetState(created1, word("0x0"), word("0x0"), (&from).into(), true,),
            AccountTouch::WarmUp(created2),
            AccountTouch::SetNonce(created1, 0, 1),
            AccountTouch::Create(
                created2,
                word("0x0"),
                word("0x1"),
                code,
                Word::from_bytes(&hash),
            ),
            AccountTouch::SetState(created2, word("0x0"), word("0x0"), word("0x42"), false),
            AccountTouch::GetState(created1, word("0x1"), word("0x0"), false),
            AccountTouch::SetState(
                created1,
                word("0x1"),
                word("0x0"),
                (&created2).into(),
                true,
            ),
            AccountTouch::FeePay(from, word("0x2e95cd937f107a2"), word("0x2e95cd937f107a2")),
        ]
    );
    Ok(())
}
