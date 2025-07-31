use solenoid::common::word::word;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    use solenoid::{
        common::{address::addr, call::Call, word::Word},
        decoder::{Bytecode, Decoder},
        eth::EthClient,
        executor::{Evm, Executor, StateTouch},
        ext::Ext,
        tracer::{EventTracer, LoggingTracer},
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
        let title = "OFFSET    PC";
        println!("{title}");
        println!("{}", "─".repeat(title.len()));
        for (src, dst) in &decoded.jumptable {
            println!("{src:#06x} -> {dst:#06x}")
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

    let code = Decoder::decode(bytecode);
    dump(&code);

    let value = Word::zero();
    // let from = addr("f39fd6e51aad88f6f4ce6ab8827279cfffb92266");
    // let to = addr("e7f1725e7734ce288f8367e1bb143e90bb3f0512")?;

    let from = addr("0xb6b1581b3d267044761156d55717b719ab0565b1");
    let to = addr("0x5c2e112783a6854653b4bc7dc22248d3e592559c");

    let call = Call {
        data: calldata,
        value,
        from,
        to,
        gas: word("0x9a38"),
    };

    let url = std::env::var("URL")?;
    let eth = EthClient::new(&url);
    let mut ext = Ext::latest(eth).await?;
    ext.acc_mut(&from).value = Word::from(1_000_000_000_000_000_000u64);

    println!("\nEXECUTION:");
    let executor = Executor::<LoggingTracer>::new().with_log();
    let mut evm = Evm::default();
    let (mut tracer, ret) = executor.execute(&code, &call, &mut evm, &mut ext).await?;
    if !evm.reverted {
        println!("OK: 0x{}", hex::encode(ret));
    } else {
        println!("REVERTED: 0x{}", hex::encode(ret));
    }

    println!("GAS: {}", evm.gas.used);
    println!("---");
    evm.state
        .iter()
        .for_each(|StateTouch(addr, key, val, new, _)| {
            if let Some(new) = new {
                println!("W:{addr}[0x{key:0x}]=0x{val:0x}->0x{new:0x}");
            } else {
                println!("R:{addr}[0x{key:0x}]=0x{val:0x}");
            }
        });
    println!("---");
    evm.account.iter().for_each(|acc| {
        use solenoid::executor::AccountTouch;
        match acc {
            AccountTouch::Empty => (),
            AccountTouch::Code(addr, hash, code) => {
                println!("CODE: [{addr}]=0x{} (0x{hash:0x})", hex::encode(code));
            }
            AccountTouch::Nonce(addr, val, new) => {
                println!("NONCE: {addr} 0x{val:0x}->0x{new:0x}");
            }
            AccountTouch::Value(addr, val, new) => {
                println!("VALUE: {addr} 0x{val:0x}->0x{new:0x}");
            }
        }
    });
    for (addr, state) in ext.state {
        println!("{addr}:");
        println!("{:#?}", state.account);
        println!("DATA: {:#?}", state.data);
        println!("CODE: ({} bytes)", state.code.0.len());
    }
    println!("---");
    let events = tracer.take();
    for event in events {
        let json = serde_json::to_string_pretty(&event).unwrap();
        println!("{json}");
    }

    Ok(())
}
