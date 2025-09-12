use evm_tracer::OpcodeTrace;

// cargo run --release --bin analyser

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let trace_path = args.first().cloned().unwrap_or_else(|| "trace.log".to_string());
    let block_path = args.get(1).cloned().unwrap_or_else(|| "block.log".to_string());

    let trace = std::fs::read_to_string(&trace_path).expect("traces");
    let trace = trace.split('\n').collect::<Vec<_>>();

    let block = std::fs::read_to_string(&block_path).expect("traces");
    let block = block.split('\n').collect::<Vec<_>>();

    if block.len() != trace.len() {
        eprintln!("WARN: len mismatch: block={} trace={}", block.len(), trace.len());
    }

    let mut matched = 0;
    for (t, b) in trace.into_iter().zip(block.into_iter()) {
        if t == b {
            matched += 1;
            continue;
        }
        if matched > 0 {
            eprintln!("WARN: skipping {matched} matching lines");
            matched = 0;
        }

        if t.is_empty() || b.is_empty() {
            break;
        }

        let t: OpcodeTrace = serde_json::from_str(t).expect("trace:json");
        let b: OpcodeTrace = serde_json::from_str(b).expect("block:json");

        let _ = std::panic::catch_unwind(|| {
            pretty_assertions::assert_eq!(b, t);
        });

        // TODO: wait for input to continue, like interactive analysis?
    }
}
