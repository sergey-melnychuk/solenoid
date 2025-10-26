use crossterm::{
    event::{KeyCode, KeyModifiers, read},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use evm_tracer::OpcodeTrace;
use serde_json::{Value, json};

fn main() -> eyre::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();

    let block_number = args
        .first()
        .and_then(|number| number.parse::<u64>().ok())
        .unwrap_or(23027350);

    let skip = args
        .get(1)
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
    let mut explore = false;
    let len = sole.len().min(revm.len());
    let mut i = 0;
    while i < len {
        let (a, b) = (revm[i], sole[i]);
        if a.is_empty() ^ b.is_empty() {
            break;
        }
        if a.starts_with('#') && b.starts_with('#') {
            i += 1;
            continue;
        }

        let a: OpcodeTrace = parse(a, &overrides);
        let b: OpcodeTrace = parse(b, &overrides);
        let r = std::panic::catch_unwind(|| {
            pretty_assertions::assert_eq!(b, a);
        });

        let is_failed = r.is_err();
        if is_failed {
            eprintln!("LINE: {i}");
            failed = true;
        } else if explore {
            let mut entry = serde_json::to_value(a.clone())?;
            entry["debug"] = json!({
                "revm": a.debug,
                "sole": b.debug,
            });
            eprintln!("{}", serde_json::to_string_pretty(&entry).unwrap());
            eprintln!("\nLINE: {i} [explore]");
        }

        if is_failed || explore {
            explore = false;
            enable_raw_mode()?;
            let event = read()?;
            disable_raw_mode()?;
            if let Some(event) = event.as_key_press_event() {
                let ctrl: bool = event.modifiers.contains(KeyModifiers::CONTROL);
                match event.code {
                    KeyCode::Char('n') => {
                        i += 1;
                        explore = !ctrl;
                        continue;
                    }
                    KeyCode::Char('p') => {
                        i -= 1;
                        explore = true;
                        continue;
                    }
                    _ => break,
                }
            }
        }

        i += 1;
        if i == len {
            explore = true;
            i -= 1;
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
