use primitive_types::U256;
use solenoid::{
    decoder::{Bytecode, Decoder},
    eth::EthClient,
    interpreter::{Call, Ext, Interpreter},
};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <bytecode> <input>", args[0]);
        std::process::exit(1);
    }

    let bytecode = hex::decode(args[1].trim_start_matches("0x"))?;
    let calldata = hex::decode(args[2].trim_start_matches("0x"))?;

    let decoded = Decoder::decode(&bytecode)?;
    dump(&decoded);

    let value = U256::from_str_radix("0", 10).unwrap();
    let from = hex::decode("ae52e300719a6d95ce1a077e939f3a51b66c22e0")
        .unwrap()
        .as_slice()
        .try_into()
        .unwrap();
    let to = hex::decode("dac17f958d2ee523a2206206994597c13d831ec7")
        .unwrap()
        .as_slice()
        .try_into()
        .unwrap();
    let call = Call {
        calldata,
        value,
        from,
        to,
    };

    let url = std::env::var("URL")?;
    let eth = EthClient::new(&url);
    let (_, block_hash) = eth.get_latest_block().await?;

    let code = eth
        .get_code(&block_hash, "0xdac17f958d2ee523a2206206994597c13d831ec7")
        .await?;
    if code != bytecode {
        eyre::bail!("bytecode inconsistency detected")
    }

    let mut ext = Ext::new(block_hash, eth);

    println!("\nEXECUTION:");

    let mut int = Interpreter::new();
    match int.execute(&decoded, &call, &mut ext).await {
        Ok(ret) => {
            println!("\nOK: 0x{}", hex::encode(ret));
        }
        Err(e) => {
            println!("\nFAILED: {e}");
        }
    }
    Ok(())
}

fn dump(decoded: &Bytecode) {
    println!("{:<6} {:<15} Argument", "PC", "OpCode");
    println!("{}", "─".repeat(40));

    for instruction in &decoded.instructions {
        let pc = format!("{:#06x}", instruction.offset);
        let opcode_name = instruction.opcode.name();
        let argument_str = if let Some(arg) = &instruction.argument {
            format!("0x{}", hex::encode(arg))
        } else {
            "".to_string()
        };
        println!("{pc:<5} {opcode_name:<15} {argument_str}");
    }

    println!("\n[JUMP  TABLE]");
    println!("OFFSET --- PC");
    println!("{}", "─".repeat(13 + 4));
    for (src, dst) in &decoded.jumptable {
        println!("{src:#06x} --- {dst:#06x}")
    }
}
