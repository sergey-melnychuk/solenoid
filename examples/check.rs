use evm_tracer::OpcodeTrace;
use serde_json::Value;

// cargo run --release --example check

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let revm_path = args
        .first()
        .cloned()
        .unwrap_or_else(|| "revm.log".to_string());
    println!("NOTE: revm path: {revm_path}");
    let sole_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "sole.log".to_string());
    println!("NOTE: sole path: {sole_path}");
    let overrides = args.get(2).cloned().unwrap_or_else(|| "{}".to_string());
    let overrides: Value = serde_json::from_str(&overrides).expect("overrides:json");
    let overrides = overrides
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();

    let revm = std::fs::read_to_string(&revm_path).expect("traces");
    let revm = revm.split('\n').collect::<Vec<_>>();

    let sole = std::fs::read_to_string(&sole_path).expect("traces");
    let sole = sole.split('\n').collect::<Vec<_>>();

    if sole.len() != revm.len() {
        eprintln!(
            "WARN: len mismatch: sole={} revm={}",
            sole.len(),
            revm.len()
        );
    }

    let mut line = 1;
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

        if r.is_err() {
            eprintln!("LINE: {line}");
            break;
        }

        line += 1;
        // TODO: wait for input to continue, like interactive analysis?
    }
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
