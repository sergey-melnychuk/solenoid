use crossterm::{event::{read, KeyCode}, terminal::{disable_raw_mode, enable_raw_mode}};
use evm_tracer::OpcodeTrace;
use serde_json::Value;

fn main() -> eyre::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();

    let block_number = args.first()
        .and_then(|number| number.parse::<u64>().ok())
        .unwrap_or(23027350);

    let skip = args.get(1)
        .and_then(|number| number.parse::<usize>().ok())
        .unwrap_or(0);

    let revm_path = format!("revm.{block_number}.{skip}.log");
    let sole_path = format!("sole.{block_number}.{skip}.log");

    let overrides = args.get(2).cloned().unwrap_or_else(|| "{}".to_string());
    let overrides: Value = serde_json::from_str(&overrides).expect("overrides:json");
    let overrides = overrides
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();

    let revm = std::fs::read_to_string(&revm_path).expect("traces");
    let revm = revm
        .split('\n')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    let sole = std::fs::read_to_string(&sole_path).expect("traces");
    let sole = sole
        .split('\n')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    if sole.len() != revm.len() {
        eprintln!(
            "WARN: len mismatch: sole={} revm={}",
            sole.len(),
            revm.len()
        );
    } else {
        eprintln!("NOTE: len match: {}", sole.len());
    }

    let mut failed = false;
    let mut line = 0;
    let pairs = revm.into_iter().zip(sole);
    for (trace, block) in pairs {
        if trace.is_empty() ^ block.is_empty() {
            break;
        }
        if trace.starts_with('#') && block.starts_with('#') {
            continue;
        }

        let trace: OpcodeTrace = parse(trace, &overrides);
        let block: OpcodeTrace = parse(block, &overrides);
        let r = std::panic::catch_unwind(|| {
            pretty_assertions::assert_eq!(block, trace);
        });

        line += 1;
        let is_failed = r.is_err();
        if is_failed {
            eprintln!("LINE: {line}");
            failed = true;
        }

        // TODO: wait for input to continue, like interactive analysis?
        if is_failed {
            enable_raw_mode()?;
            let event = read()?;
            disable_raw_mode()?;
            if let Some(event) = event.as_key_press_event() {
                match event.code {
                    KeyCode::Char('n') => continue,
                    _ => break
                }
            }
        }
    }

    if !failed {
        println!("OK");
    }
    Ok(())
}

fn parse(s: &str, overrides: &[(String, Value)]) -> OpcodeTrace {
    let mut json: Value = serde_json::from_str(s).expect("opcode:json");
    for (name, value) in overrides {
        if let Some(field) = json.get_mut(name) {
            *field = value.clone();
        }
    }
    serde_json::from_value(json).expect("opcode:parse")
}
