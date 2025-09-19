use evm_tracer::OpcodeTrace;
use serde_json::Value;

// cargo run --release --bin check

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let trace_path = args
        .first()
        .cloned()
        .unwrap_or_else(|| "trace.log".to_string());
    println!("NOTE: trace path: {trace_path}");
    let block_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "block.log".to_string());
    println!("NOTE: block path: {block_path}");
    let overrides = args.get(2).cloned().unwrap_or_else(|| "{}".to_string());
    let overrides: Value = serde_json::from_str(&overrides).expect("overrides:json");
    let overrides = overrides
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();

    let trace = std::fs::read_to_string(&trace_path).expect("traces");
    let trace = trace.split('\n').collect::<Vec<_>>();

    let block = std::fs::read_to_string(&block_path).expect("traces");
    let block = block.split('\n').collect::<Vec<_>>();

    if block.len() != trace.len() {
        eprintln!(
            "WARN: len mismatch: block={} trace={}",
            block.len(),
            trace.len()
        );
    }

    let mut line = 1;
    let pairs = trace.into_iter().zip(block);
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
