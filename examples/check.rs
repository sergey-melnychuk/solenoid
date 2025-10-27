use crossterm::{
    event::{read, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use evm_tracer::OpcodeTrace;
use serde_json::{Value, json};

enum Predicate {
    Depth(usize),
    IsCall,
    None,
}

impl Predicate {
    fn check(&self, trace: &OpcodeTrace) -> bool {
        match self {
            Self::Depth(d) => &trace.depth == d,
            Self::IsCall => trace.debug.value["is_call"].as_bool().unwrap_or_default(),
            Self::None => false,
        }
    }
}

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

    let is_compact = args.iter().skip(2).any(|arg| arg == "--compact");

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

    let mut p = Predicate::None;

    let mut failed = false;
    let mut explore = false;
    let len = sole.len().min(revm.len()) as i64;
    let mut index: i64 = 0;
    let mut step = 1i64;
    while index < len {
        let i = index as usize;
        let (a, b) = (revm[i], sole[i]);
        if a.is_empty() ^ b.is_empty() {
            break;
        }

        let mut a: OpcodeTrace = parse(a, &overrides);
        let mut b: OpcodeTrace = parse(b, &overrides);
        if is_compact {
            if a.memory == b.memory {
                a.memory.clear();
                b.memory.clear();
            }
            if a.stack == b.stack {
                a.stack.clear();
                b.stack.clear();
            }
        }
        let r = std::panic::catch_unwind(|| {
            pretty_assertions::assert_eq!(b, a);
        });

        let is_failed = r.is_err();
        if is_failed && matches!(p, Predicate::None) {
            eprintln!("LINE: {i}");
            failed = true;
        } else if explore || p.check(&b) {
            let mut entry = serde_json::to_value(a.clone())?;
            entry["debug"] = json!({
                "revm": a.debug,
                "sole": b.debug,
            });
            eprintln!("{}", serde_json::to_string_pretty(&entry).unwrap());
            eprintln!("\nLINE: {i} [explore]");
        }

        if is_failed || explore || p.check(&b) {
            explore = false;
            enable_raw_mode()?;
            let event = read()?;
            disable_raw_mode()?;
            if let Some(event) = event.as_key_press_event() {
                let shift: bool = event.modifiers.contains(KeyModifiers::SHIFT);
                match event.code {
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        step = 1;
                        explore = !shift;
                        p = Predicate::None;
                    }
                    KeyCode::Char('p') => {
                        step = -1;
                        explore = true;
                        p = Predicate::None;
                    }
                    KeyCode::Char('d') => {
                        p = Predicate::Depth(b.depth + 1);
                        explore = false;
                        step = 1;
                    }
                    KeyCode::Char('c') => {
                        p = Predicate::IsCall;
                        explore = false;
                        step = 1;
                    }
                    _ => break,
                }
            }
        }

        if !failed && index == len-1 && step > 0 {
            explore = true;
        }
        if index == 0 && step < 0 {
            break;
        }
        index += step;
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
