use evm_common::{address::addr, word::Word};
use solenoid::{
    eth,
    ext::{Account, Ext},
    solenoid::{Builder, Solenoid},
};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let aa = addr("0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    let bb = addr("0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB");
    let cc = addr("0xCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC");

    let url = std::env::var("URL")?;
    let eth = eth::EthClient::new(&url);
    let mut ext = Ext::at_latest(eth).await?;

    let one = Word::from(10u64.pow(18));
    let two = Word::from(2 * 10u64.pow(18));
    ext.state.insert(aa, Account { value: two, ..Default::default() });
    ext.state.insert(bb, Account { value: two, ..Default::default() });
    ext.state.insert(cc, Account { value: two, ..Default::default() });

    let code = include_str!("../etc/reentrancy/Vulnerable.bin");
    let code = hex::decode(code.trim_start_matches("0x"))?;
    let r = Solenoid::new()
        .create(code)
        .with_sender(cc)
        .with_gas(Word::from(1_000_000))
        .with_value(Word::zero())
        .ready()
        .apply(&mut ext)
        .await?;
    println!("Target: OK={:#?}", !r.evm.reverted);

    let target = ext
        .created_accounts
        .first()
        .copied()
        .ok_or_else(|| eyre::eyre!("No address returned"))?;
    println!("Target: {target}");

    let expected = include_str!("../etc/reentrancy/Vulnerable.bin-runtime");
    let expected = hex::decode(expected.trim_start_matches("0x"))?;
    assert_eq!(ext.code(&target).await?.0.len(), expected.len(), "target code mismatch");

    let code = include_str!("../etc/reentrancy/Attacker.bin");
    let code = hex::decode(code.trim_start_matches("0x"))?;
    let r = Solenoid::new()
        .create(code)
        .with_sender(aa)
        .with_gas(Word::from(1_000_000))
        .with_value(Word::zero())
        .ready()
        .apply(&mut ext)
        .await?;
    println!("Attack: OK={}", !r.evm.reverted);

    let attack = ext
        .created_accounts
        .get(1)
        .copied()
        .ok_or_else(|| eyre::eyre!("No address returned"))?;
    println!("Attack: {attack}");

    let expected = include_str!("../etc/reentrancy/Attacker.bin-runtime");
    let expected = hex::decode(expected.trim_start_matches("0x"))?;
    assert_eq!(ext.code(&attack).await?.0.len(), expected.len(), "attack code mismatch");

    println!("---");

    let _ = Solenoid::new()
        .execute(target, "deposit()", &[])
        .with_sender(bb)
        .with_gas(Word::from(1_000_000))
        .with_value(one * Word::from(8))
        .ready()
        .apply(&mut ext)
        .await?;

    let balance = ext.balance(&target).await?;
    println!("Target balance: {}", format_eth(&balance));
    let balance = ext.balance(&attack).await?;
    println!("Attack balance: {}", format_eth(&balance));

    println!("---");

    let r = Solenoid::new()
        .execute(attack, "attack(address)", &target.as_word().into_bytes())
        .with_sender(aa)
        .with_gas(Word::from(1_000_000))
        .with_value(one)
        .ready()
        .apply(&mut ext)
        .await?;
    println!("Attack: OK={:#?} {}", !r.evm.reverted, format_eth(&one));

    let balance = ext.balance(&target).await?;
    println!("Target balance: {}", format_eth(&balance));
    let balance = ext.balance(&attack).await?;
    println!("Attack balance: {}", format_eth(&balance));

    Ok(())
}

fn format_eth(word: &Word) -> String {
    let base = Word::from(10u64.pow(18));
    let before = *word / base;
    let after = *word % base;
    format!("{}.{} ETH", before.as_u64(), after.as_u64())
}