use std::{fs::File, io::{BufRead, BufReader}};

use crossterm::{
    event::{KeyCode, KeyModifiers, read},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use evm_tracer::OpcodeTrace;
use serde_json::json;

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

    let revm_state = std::fs::read_to_string(&format!("revm.{block_number}.{skip}.state.json"))?;
    let sole_state = std::fs::read_to_string(&format!("sole.{block_number}.{skip}.state.json"))?;

    let revm_state = serde_json::from_str::<serde_json::Value>(&revm_state)?;
    let sole_state = serde_json::from_str::<serde_json::Value>(&sole_state)?;

    let r = std::panic::catch_unwind(|| {
        pretty_assertions::assert_eq!(sole_state, revm_state);
    });
    let state_ok = r.is_ok();
    if state_ok {
        eprintln!("STATE: OK");
    } else {
        eprintln!("STATE: MISMATCH");
    }

    let revm_path = format!("revm.{block_number}.{skip}.log");
    let sole_path = format!("sole.{block_number}.{skip}.log");

    let is_compact = args.iter().skip(2).any(|arg| arg == "--compact");

    let non_interactive = args.iter().skip(2).any(|arg| arg == "--noninteractive");

    let revm = BufReader::new(File::open(&revm_path)?);
    let sole = BufReader::new(File::open(&sole_path)?);
    let iter = revm.lines().zip(sole.lines());

    let mut p = Predicate::None;

    let mut failed = false;
    let mut explore = false;
    let mut index: i64 = 0;
    let mut step = 1i64;
    for (revm, sole) in iter {
        let i = index as usize;
        if !non_interactive {
            if i % 1000 == 0 { use std::io::Write; print!("\r(check: {i})"); std::io::stdout().flush().unwrap(); }
        }
        let (a, b) = (revm?, sole?);
        if a.is_empty() ^ b.is_empty() {
            break;
        }

        let mut a: OpcodeTrace = serde_json::from_str(&a).unwrap();
        let mut b: OpcodeTrace = serde_json::from_str(&b).unwrap();
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

        if r.is_err() && non_interactive {
            let suffix = if a.gas_left <= 0 || b.gas_left <= 0 {
                format!(
                    ",[out-of-gas]revm.gas_left={},sole.gas_left={}",
                    a.gas_left, b.gas_left
                )
            } else if b.name == "BALANCE" {
                format!(
                    ",revm.stack[0]=0x{},sole.stack[0]=0x{}",
                    a.stack[0].replace("00", ""),
                    b.stack[0].replace("00", ""),
                )
            } else {
                String::from("")
            };
            println!("{block_number} {skip} LINE={i},pc={},op={}{suffix}", b.pc, b.name);
            return Ok(());
        }

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

        if !non_interactive && (is_failed || explore || p.check(&b)) {
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
                    KeyCode::Char('p') | KeyCode::Char('P') => {
                        step = -1;
                        explore = !shift;
                        p = Predicate::None;
                    }
                    KeyCode::Char('d') | KeyCode::Char('D') => {
                        p = Predicate::Depth(b.depth + 1);
                        explore = false;
                        step = if !shift { 1 } else { -1 };
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        p = Predicate::IsCall;
                        explore = false;
                        step = if !shift { 1 } else { -1 };
                    }
                    KeyCode::Char('g') | KeyCode::Char('G') => {
                        explore = true;
                        if shift {
                        } else {
                            index = 0;
                        };
                        step = 0;
                        p = Predicate::None;
                    }
                    _ => break,
                }
            }
        }

        if index == 0 && step < 0 {
            break;
        }
        index += step;
    }

    if !failed {
        if non_interactive {
            println!("{block_number} {skip} OK");
        } else {
            println!("OK");
        }
    }
    Ok(())
}
