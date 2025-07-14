#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> eyre::Result<()> {
    use primitive_types::U256;
    use solenoid::{
        decoder::{Bytecode, Decoder},
        eth::EthClient,
        executor::{Call, Executor, Ext, NoopTracer},
    };

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

        println!("\n[JUMP TABLE]");
        println!("OFFSET -- PC");
        println!("{}", "─".repeat(13 + 4));
        for (src, dst) in &decoded.jumptable {
            println!("{src:#06x} --- {dst:#06x}")
        }
    }

    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <bytecode> <input>", args[0]);
        std::process::exit(1);
    }

    let bytecode = hex::decode(args[1].trim_start_matches("0x"))?;
    let calldata = hex::decode(args[2].trim_start_matches("0x"))?;

    let code = Decoder::decode(&bytecode)?;
    dump(&code);

    let value = U256::zero();
    let from = hex::decode("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266")
        .unwrap()
        .as_slice()
        .try_into()
        .unwrap();
    let to = hex::decode("e7f1725E7734CE288F8367e1Bb143E90bb3F0512")
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

    let mut ext = Ext::new(block_hash, eth);

    println!("\nEXECUTION:");

    let executor = Executor::<NoopTracer>::new();
    let (_, evm, ret) = executor.execute(&bytecode, &code, &call, &mut ext).await?;
    if !evm.reverted {
        println!("\nOK: 0x{}", hex::encode(ret));
    } else {
        println!("\nFAILED: reverted");
    }

    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
