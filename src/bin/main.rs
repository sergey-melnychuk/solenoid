use primitive_types::U256;
use solenoid::{
    decoder::{DecodedBytecode, Decoder},
    interpreter::{Call, Interpreter},
};
use std::{env, process};

fn main() -> eyre::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <bytecode> <input>", args[0]);
        process::exit(1);
    }

    let bytecode = hex::decode(args[1].trim_start_matches("0x"))?;
    let calldata = hex::decode(args[2].trim_start_matches("0x"))?;

    let decoded = Decoder::new(&bytecode).decode()?;
    dump(&decoded);

    let value = U256::from_str_radix("0", 10).unwrap();
    let address = hex::decode("be862ad9abfe6f22bcb087716c7d89a26051f74c").unwrap();
    let call = Call {
        calldata,
        value,
        from: address.as_slice().try_into().unwrap(),
    };
    let mut int = Interpreter::new(&decoded, &call);
    println!("\nEXECUTION:");
    let ret = int.execute(&call)?;
    println!("\nRET: 0x{}", hex::encode(ret));

    Ok(())
}

fn dump(decoded: &DecodedBytecode) {
    println!("{:<5} {:<15} Argument", "PC", "OpCode");
    println!("{}", "─".repeat(40));

    for instruction in &decoded.instructions {
        let pc = format!("{:#04x}", instruction.offset);
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
