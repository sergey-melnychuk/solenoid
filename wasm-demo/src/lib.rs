use wasm_bindgen::prelude::*;
use solenoid::{
    common::{
        address::{Address, addr},
        hash::keccak256,
        word::Word,
    },
    eth::EthClient,
    ext::Ext,
    solenoid::{Builder, Solenoid}, tracer::{EventData, EventTracer},
};

// TODO: lookup Tx by hash, replay by solenoid

// TODO: build nice stack-trace of calls in the Tx

// TODO: pull tx receipt and validate gas usage?

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub async fn get_latest_block_number(rpc_url: String) -> Result<String, JsValue> {
    let client = EthClient::new(&rpc_url);

    match client.get_latest_block().await {
        Ok((block_number, block_hash)) => {
            Ok(format!("Block Number: {}\nBlock Hash: {}", block_number, block_hash))
        }
        Err(e) => Err(JsValue::from_str(&format!("Error: {}", e))),
    }
}

#[wasm_bindgen]
pub async fn quote_weth_to_usdc(
    rpc_url: String,
    amount_weth: String,
) -> Result<String, JsValue> {
    // Parse the amount (in WETH with 18 decimals)
    let amount_in: u128 = amount_weth
        .parse()
        .map_err(|e| JsValue::from_str(&format!("Invalid amount: {}", e)))?;
    let amount_in = Word::from(amount_in);

    // Constants
    const UNISWAP_V3_QUOTER: Address = addr("0x61fFE014bA17989E743c5F6cB21bF9697530B21e"); // Quoter V2
    let weth_address = addr("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
    let usdc_address = addr("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");

    // Prepare the call to quoteExactInputSingle
    let method = "quoteExactInputSingle((address,address,uint256,uint24,uint160))";
    let selector = &keccak256(method.as_bytes())[..4];

    // Build calldata for the quoter call
    let mut calldata = Vec::new();
    calldata.extend_from_slice(selector);
    calldata.extend_from_slice(&weth_address.as_word().into_bytes()); // tokenIn (WETH)
    calldata.extend_from_slice(&usdc_address.as_word().into_bytes()); // tokenOut (USDC)
    calldata.extend_from_slice(&amount_in.into_bytes()); // amountIn
    calldata.extend_from_slice(&Word::from(3_000).into_bytes()); // fee (3000 basis points = 0.3%)
    calldata.extend_from_slice(&Word::zero().into_bytes()); // sqrtPriceLimitX96 (0 for no limit)

    // Initialize Ethereum client
    let eth = EthClient::new(&rpc_url);

    // Make eth_call using the client
    let result_hex = eth
        .eth_call(&UNISWAP_V3_QUOTER, &calldata)
        .await
        .map_err(|e| {
            let error_msg = format!("eth_call failed: {}", e);
            web_sys::console::log_1(&format!("Debug - eth_call error: {}", e).into());
            JsValue::from_str(&error_msg)
        })?;

    web_sys::console::log_1(&format!("Debug - got result: {}", result_hex).into());

    let result_bytes = hex::decode(result_hex.trim_start_matches("0x"))
        .map_err(|e| JsValue::from_str(&format!("Debug - error decoding result hex: {}", e)))?;

    web_sys::console::log_1(&format!("Debug - got result: {} bytes", result_bytes.len()).into());

    // Decode the result
    if result_bytes.len() >= 128 {
        let amount_out = Word::from_bytes(&result_bytes[0..32]);
        let sqrt_price_x96_after = Word::from_bytes(&result_bytes[32..64]);
        let initialized_ticks_crossed = Word::from_bytes(&result_bytes[64..96]);
        let gas_estimate = Word::from_bytes(&result_bytes[96..128]);

        // Calculate price
        let weth_decimals = 18;
        let usdc_decimals = 6;
        let price_after = calculate_price_from_sqrt(sqrt_price_x96_after, usdc_decimals, weth_decimals);

        // Format amounts
        let weth_amount = amount_in.as_u128() as f64 / 1e18;
        let usdc_amount = amount_out.as_u128() as f64 / 1e6;

        Ok(format!(
            "Quote Results:\n\
             Amount In: {} WETH\n\
             Amount Out: {} USDC\n\
             Price (WETH/USDC): {:.2}\n\
             Ticks Crossed: {}\n\
             Gas Estimate: {}\n\
             Method: eth_call",
            weth_amount,
            usdc_amount,
            1.0 / price_after,
            initialized_ticks_crossed,
            gas_estimate.as_u64()
        ))
    } else {
        Err(JsValue::from_str(&format!(
            "Unexpected return data length: {} bytes (expected at least 128). Got hex: {}",
            result_bytes.len(),
            hex::encode(result_bytes)
        )))
    }
}

#[wasm_bindgen]
pub async fn quote_weth_to_usdc_solenoid(
    rpc_url: String,
    amount_weth: String,
) -> Result<String, JsValue> {
    // Parse the amount (in WETH with 18 decimals)
    let amount_in: u128 = amount_weth
        .parse()
        .map_err(|e| JsValue::from_str(&format!("Invalid amount: {}", e)))?;
    let amount_in = Word::from(amount_in);

    // Constants
    const UNISWAP_V3_QUOTER: Address = addr("0x61fFE014bA17989E743c5F6cB21bF9697530B21e"); // Quoter V2
    let weth_address = addr("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
    let usdc_address = addr("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");
    let from = addr("0xb18f13b8fde294e0147188a78d5b1328f206f4e2");

    web_sys::console::log_1(&"Debug - Starting Solenoid execution".into());

    // Initialize Ethereum client
    let eth = EthClient::new(&rpc_url);

    // Get latest block
    let (latest_block_number, _) = eth
        .get_latest_block()
        .await
        .map_err(|e| JsValue::from_str(&format!("Failed to get latest block: {}", e)))?;

    web_sys::console::log_1(&format!("Debug - Latest block: {}", latest_block_number).into());

    let header = eth
        .get_block_header(Word::from(latest_block_number))
        .await
        .map_err(|e| JsValue::from_str(&format!("Failed to get block header: {}", e)))?;

    web_sys::console::log_1(&"Debug - Got block header".into());

    let mut ext = Ext::at_number(Word::from(latest_block_number - 1), eth)
        .await
        .map_err(|e| JsValue::from_str(&format!("Failed to create Ext: {}", e)))?;

    web_sys::console::log_1(&format!("Debug - Created Ext at block {}", latest_block_number - 1).into());

    // Prepare the call to quoteExactInputSingle
    let method = "quoteExactInputSingle((address,address,uint256,uint24,uint160))";

    // Build arguments for the quoter call
    let mut args = Vec::new();
    args.extend_from_slice(&weth_address.as_word().into_bytes()); // tokenIn (WETH)
    args.extend_from_slice(&usdc_address.as_word().into_bytes()); // tokenOut (USDC)
    args.extend_from_slice(&amount_in.into_bytes()); // amountIn
    args.extend_from_slice(&Word::from(3_000).into_bytes()); // fee (3000 basis points = 0.3%)
    args.extend_from_slice(&Word::zero().into_bytes()); // sqrtPriceLimitX96 (0 for no limit)

    web_sys::console::log_1(&format!("Debug - Built {} bytes of calldata args", args.len()).into());
    web_sys::console::log_1(&"Debug - Executing with Solenoid...".into());

    // Execute the quoter call using Solenoid
    let sole = Solenoid::new();

    // Build the execution
    let runner = sole
        .execute(UNISWAP_V3_QUOTER, method, &args)
        .with_header(header)
        .with_sender(from)
        .with_gas(Word::from(1_000_000))
        .ready();

    web_sys::console::log_1(&"Debug - Runner ready, starting execution...".into());

    // Execute
    let mut result = runner
        .apply(&mut ext)
        .await
        .map_err(|e| {
            let error_string = format!("{:?}", e);
            web_sys::console::log_1(&format!("Debug - Execution error details: {}", error_string).into());
            JsValue::from_str(&format!("Execution failed: {}", error_string))
        })?;

    web_sys::console::log_1(&format!("Debug - Got {} bytes of return data", result.ret.len()).into());

    let traces = result.tracer.take();
    web_sys::console::log_1(&format!("Solenoid - traces: {}", traces.len()).into());
    for event in traces {
        let keep = matches!(event.data, 
            EventData::Call { .. } | EventData::Return { .. } | EventData::State(_) | EventData::Account(_));
        if keep {
            let json = serde_json_wasm::to_string(&event).unwrap();
            web_sys::console::log_1(&json.into());
        }
    }

    // Decode the result
    if result.ret.len() >= 128 {
        let amount_out = Word::from_bytes(&result.ret[0..32]);
        let sqrt_price_x96_after = Word::from_bytes(&result.ret[32..64]);
        let initialized_ticks_crossed = Word::from_bytes(&result.ret[64..96]);
        let gas_estimate = Word::from_bytes(&result.ret[96..128]);

        // Calculate price
        let weth_decimals = 18;
        let usdc_decimals = 6;
        let price_after = calculate_price_from_sqrt(sqrt_price_x96_after, usdc_decimals, weth_decimals);

        // Format amounts
        let weth_amount = amount_in.as_u128() as f64 / 1e18;
        let usdc_amount = amount_out.as_u128() as f64 / 1e6;

        Ok(format!(
            "Quote Results:\n\
             Amount In: {} WETH\n\
             Amount Out: {} USDC\n\
             Price (WETH/USDC): {:.2}\n\
             Ticks Crossed: {}\n\
             Gas Estimate: {}\n\
             Method: Solenoid",
            weth_amount,
            usdc_amount,
            1.0 / price_after,
            initialized_ticks_crossed,
            gas_estimate.as_u64()
        ))
    } else {
        Err(JsValue::from_str(&format!(
            "Unexpected return data length: {} bytes (expected at least 128)",
            result.ret.len()
        )))
    }
}

fn calculate_price_from_sqrt(
    sqrt_price_x96: Word,
    decimals_token0: u8,
    decimals_token1: u8,
) -> f64 {
    let sqrt_price_x96_f64 = sqrt_price_x96.as_u128() as f64;
    let q96 = 2_f64.powi(96);
    let sqrt_price = sqrt_price_x96_f64 / q96;
    let raw_price = sqrt_price * sqrt_price;
    let decimal_adjustment = 10_f64.powi(decimals_token0 as i32 - decimals_token1 as i32);
    raw_price * decimal_adjustment
}
