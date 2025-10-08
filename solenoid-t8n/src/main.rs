use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[arg(long = "input.alloc")]
    input_alloc: PathBuf,

    #[arg(long = "input.txs")]
    input_txs: PathBuf,

    #[arg(long = "input.env")]
    input_env: PathBuf,

    #[arg(long = "output.basedir")]
    output_basedir: PathBuf,

    #[arg(long = "output.result")]
    output_result: PathBuf,

    #[arg(long = "output.alloc")]
    output_alloc: PathBuf,

    #[arg(long = "output.body")]
    output_body: Option<PathBuf>,

    #[arg(long = "state.fork")]
    fork: String,

    #[arg(long = "state.chainid")]
    chain_id: u64,

    #[arg(long = "state.reward", default_value = "0")]
    reward: u64,

    #[arg(long = "trace")]
    trace: bool,
}

#[derive(Deserialize)]
struct Alloc {
    // Your pre-state structure
}

#[derive(Deserialize)]
struct Env {
    #[serde(rename = "currentCoinbase")]
    coinbase: String,
    #[serde(rename = "currentDifficulty")]
    difficulty: String,
    #[serde(rename = "currentGasLimit")]
    gas_limit: String,
    #[serde(rename = "currentNumber")]
    number: String,
    #[serde(rename = "currentTimestamp")]
    timestamp: String,
    #[serde(rename = "currentBaseFee")]
    base_fee: Option<String>,
    // Add other fields as needed
}

#[derive(Debug, Deserialize)]
struct JsonTransaction {
    #[serde(rename = "type")]
    tx_type: Option<String>,
    #[serde(rename = "chainId")]
    chain_id: Option<String>,
    nonce: String,
    #[serde(rename = "gasPrice")]
    gas_price: Option<String>,
    gas: String,
    to: Option<String>,
    value: String,
    input: String,
    v: String,
    r: String,
    s: String,
    // Optional fields we can ignore for now
    sender: Option<String>,
    #[serde(rename = "secretKey")]
    secret_key: Option<String>,
    // EIP-1559 fields
    #[serde(rename = "maxPriorityFeePerGas")]
    max_priority_fee_per_gas: Option<String>,
    #[serde(rename = "maxFeePerGas")]
    max_fee_per_gas: Option<String>,
    // EIP-2930 field
    #[serde(rename = "accessList")]
    access_list: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct Result {
    #[serde(rename = "stateRoot")]
    state_root: String,
    #[serde(rename = "txRoot")]
    tx_root: String,
    #[serde(rename = "receiptsRoot")]
    receipt_root: String,
    #[serde(rename = "logsHash")]
    logs_hash: String,
    #[serde(rename = "logsBloom")]
    logs_bloom: String,
    receipts: Vec<Receipt>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    rejected: Vec<RejectedTx>,
    #[serde(rename = "currentDifficulty", skip_serializing_if = "Option::is_none")]
    current_difficulty: Option<String>,
    #[serde(rename = "gasUsed")]
    gas_used: String,
    base_fee_per_gas: String,
    withdrawals_root: String,
}

#[derive(Serialize)]
struct Receipt {
    root: Option<String>,
    status: Option<String>,
    #[serde(rename = "cumulativeGasUsed")]
    cumulative_gas_used: String,
    #[serde(rename = "logsBloom")]
    logs_bloom: String,
    logs: Option<Vec<Log>>,
    #[serde(rename = "transactionHash")]
    transaction_hash: String,
    #[serde(rename = "contractAddress")]
    contract_address: Option<String>,
    #[serde(rename = "gasUsed")]
    gas_used: String,
    #[serde(rename = "blockHash")]
    block_hash: String,
    #[serde(rename = "transactionIndex")]
    transaction_index: String,
}

#[derive(Serialize)]
struct Log {
    address: String,
    topics: Vec<String>,
    data: String,
}

#[derive(Serialize)]
struct RejectedTx {
    index: usize,
    error: String,
}

fn load_transactions(txs_path: &PathBuf) -> eyre::Result<Vec<JsonTransaction>> {
    let content = fs::read_to_string(txs_path)?;

    // Try parsing as JSON first
    if let Ok(txs) = serde_json::from_str::<Vec<JsonTransaction>>(&content) {
        eprintln!("DEBUG: Loaded {} JSON transactions", txs.len());
        return Ok(txs);
    }

    // If not JSON, it might be RLP (handle later)
    eyre::bail!("Failed to parse transactions as JSON")
}

/*
solenoid-t8n \
  --input.alloc=<alloc.json> \    # Pre-state
  --input.txs=<txs.rlp> \         # Transactions (RLP encoded)
  --input.env=<env.json> \        # Block environment
  --output.basedir=<dir> \
  --output.result=<result.json> \
  --output.alloc=<alloc-out.json> \
  --state.fork=<ForkName>         # e.g., Shanghai, Cancun
*/
fn main() -> eyre::Result<()> {
    let v = std::env::args().skip(1).next().unwrap_or_default();
    if v == "-v" || v == "--version" {
        println!("evm version 1.14.0-stable"); // Pretend to be Geth
        std::process::exit(0);
    }

    let args = Args::parse();
    eprintln!("solenoid-t8n");

    // Load inputs
    let alloc_json = fs::read_to_string(&args.input_alloc)?;
    let alloc: Alloc = serde_json::from_str(&alloc_json)?;

    let env_json = fs::read_to_string(&args.input_env)?;
    let env: Env = serde_json::from_str(&env_json)?;

    let txs = load_transactions(&args.input_txs)?;

    // Initialize your solenoid EVM with the fork
    // let mut evm = solenoid::Evm::new(&args.fork)?;
    // let mut state = solenoid::State::from_alloc(alloc);

    // Execute transactions
    // let (result, state) = evm.execute_block(&mut state, txs_rlp, env)?;

    // For now, create a dummy result (replace with actual execution)
    let result = Result {
        state_root: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        tx_root: "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421".to_string(),
        receipt_root: "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421".to_string(),
        logs_hash: "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347".to_string(),
        logs_bloom: "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000".to_string(),
        receipts: vec![Receipt {
            // For post-Byzantium, use status instead of root
            root: None,
            status: Some("0x1".to_string()), // 0x1 = success, 0x0 = failure
            cumulative_gas_used: format!("0x12345"),
            // Empty logs bloom (256 bytes of zeros)
            logs_bloom: "0x".to_string() + &"00".repeat(256),
            // Empty logs array
            logs: Some(vec![]),
            transaction_hash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            // Contract address only if this was a contract creation (to == None)
            contract_address: Some("0x0000000000000000000000000000000000000000".to_string()),
            gas_used: "0x5208".to_string(), // 21000 gas (minimum for a tx)
            block_hash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),            
            transaction_index: format!("0x42"),
        }],
        rejected: vec![],
        current_difficulty: Some("0x0".to_string()),
        gas_used: "0x0".to_string(),
        base_fee_per_gas: env.base_fee.unwrap_or("0x1".to_string()),
        withdrawals_root: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    };

    // Create the output directory if it doesn't exist
    if !fs::exists(&args.output_basedir)? {
        fs::create_dir_all(&args.output_basedir)?;
    }

    eprintln!("output_result={:?}", args.output_result);
    eprintln!("output_alloc={:?}", args.output_alloc);
    eprintln!("{}", serde_json::to_string_pretty(&result)?);

    // Write result
    let output_result = args.output_basedir.join(args.output_result);
    fs::write(&output_result, serde_json::to_string_pretty(&result)?)?;

    let state = serde_json::json!({});

    // Write post-state alloc
    let output_alloc = args.output_basedir.join(args.output_alloc);
    fs::write(output_alloc, serde_json::to_string_pretty(&state)?)?;

    Ok(())
}
