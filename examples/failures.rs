use eyre::Context;
use solenoid::{
    common::{address::addr, word::Word},
    ext::Ext,
    solenoid::{Builder, Solenoid},
    tracer::EventTracer,
};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt::init();

    let from = addr("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512");

    let mut ext = Ext::local();
    ext.acc_mut(&from).balance = Word::from(100_000_000_000_000_000u64);
    ext.data_mut(&from.of_smart_contract(Word::zero()))
        .insert(Word::zero(), Word::zero()); // Fail.owner

    let code = include_str!("../etc/fail/Fail.bin");
    let code = hex::decode(code.trim_start_matches("0x"))?;

    let sole = Solenoid::new();
    let mut res = sole
        .create(code)
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .with_value(Word::zero())
        .ready()
        .apply(&mut ext)
        .await
        .context("create")?;
    for e in res.tracer.take() {
        println!("{}", serde_json::to_string(&e).unwrap());
    }
    let address = res
        .created
        .ok_or_else(|| eyre::eyre!("No address returned"))?;
    println!(" Owner: {from}");
    println!("Deploy: {address}");
    assert_eq!(address, from.of_smart_contract(Word::zero()));

    let mut res = sole
        .execute(address, "is_owner()", &[])
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    for e in res.tracer.take() {
        println!("{}", serde_json::to_string(&e).unwrap());
    }
    let ret = if res.evm.reverted {
        format!("FAILURE: '{}'", decode_error_string(&res.ret))
    } else {
        format!("SUCCESS: '{}'", hex::encode(res.ret))
    };
    println!("Fail.is_owner({from}): {ret}");

    let user = from.of_smart_contract(Word::one());
    let mut res = sole
        .execute(address, "is_owner()", &[])
        .with_sender(user)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    for e in res.tracer.take() {
        println!("{}", serde_json::to_string(&e).unwrap());
    }
    let ret = if res.evm.reverted {
        format!("FAILURE: '{}'", decode_error_string(&res.ret))
    } else {
        format!("SUCCESS: '{}'", hex::encode(res.ret))
    };
    println!("Fail.is_owner({user}): {ret}");

    let number = 0xffu8 - 1;
    let mut arg = [0u8; 32];
    arg[31] = number;
    let mut res = sole
        .execute(address, "even_only(uint8)", &arg)
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    for e in res.tracer.take() {
        println!("{}", serde_json::to_string(&e).unwrap());
    }
    let ret = if res.evm.reverted {
        format!("FAILURE: '{}'", decode_error_string(&res.ret))
    } else {
        format!("SUCCESS: '{}'", hex::encode(res.ret))
    };
    println!("Fail.even_only({number}): {ret}");

    let number = 0xffu8;
    let mut arg = [0u8; 32];
    arg[31] = number;
    let mut res = sole
        .execute(address, "even_only(uint8)", &arg)
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready()
        .apply(&mut ext)
        .await
        .context("execute")?;
    for e in res.tracer.take() {
        println!("{}", serde_json::to_string(&e).unwrap());
    }
    let ret = if res.evm.reverted {
        format!("FAILURE: '{}'", decode_error_string(&res.ret))
    } else {
        format!("SUCCESS: '{}'", hex::encode(res.ret))
    };
    println!("Fail.even_only({number}): {ret}");

    Ok(())
}

fn decode_error_string(ret: &[u8]) -> String {
    let _selector = &ret[0..4];
    let offset = 4 + 32 + Word::from_bytes(&ret[4..4 + 32]).as_usize();
    let size = Word::from_bytes(&ret[4 + 32..4 + 32 + 32]).as_usize();

    let data = &ret[offset..offset + size];
    String::from_utf8_lossy(data).to_string()
}
