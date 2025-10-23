# Solenoid WASM Demo

A WebAssembly demo that showcases the Solenoid EVM executor running in the browser. This demo includes:

1. **Block Number Fetcher**: Fetches the latest Ethereum block number
2. **Uniswap V3 Quoter**: Simulates WETH to USDC swaps using the Uniswap V3 Quoter contract

## Features

### 1. Get Latest Block Number
Simple RPC call to fetch the latest block number and hash from an Ethereum node.

### 2. Quote WETH to USDC (via eth_call)
This demo calls the Uniswap V3 QuoterV2 contract using a simple `eth_call` RPC request:
- Expected output amount (USDC)
- Current price after the swap
- Number of initialized ticks crossed
- Gas estimate for the swap

**Method:** Uses direct RPC `eth_call` - fast and lightweight.

### 3. Quote WETH to USDC (via Solenoid)
This demo uses the **full Solenoid EVM executor** compiled to WASM to simulate the swap:
- Expected output amount (USDC)
- Current price after the swap
- Number of initialized ticks crossed
- Gas estimate from the quoter contract
- **Total gas used by Solenoid** (includes full EVM execution)
- Reverted status

**Method:** Runs the complete EVM transaction locally in your browser, fetching state from the RPC as needed. This showcases the true power of Solenoid!

**Technical Details:**
- Uses Uniswap V3 QuoterV2 at `0x61fFE014bA17989E743c5F6cB21bF9697530B21e`
- WETH address: `0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2`
- USDC address: `0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48`
- Pool fee: 0.3% (3000 basis points)

## Build and Run

1. Install wasm-pack if you haven't already:
```bash
cargo install wasm-pack
```

2. Build the WASM package:
```bash
cd wasm-demo
wasm-pack build --target web
```

3. Serve the page with a local HTTP server (required for ES modules):
```bash
# Using Python
python3 -m http.server 8000

# Or using Node.js
npx http-server -p 8000
```

4. Open your browser to http://localhost:8000

## Usage

### Fetch Latest Block
1. Enter an Ethereum RPC URL (default is https://eth.llamarpc.com)
2. Click "Fetch Latest Block"
3. View the current block number and hash

### Get WETH/USDC Quote
1. Enter an Ethereum RPC URL
2. Enter the amount in Wei (use presets for convenience):
   - 1 WETH = 1000000000000000000 Wei
   - 0.5 WETH = 500000000000000000 Wei
   - 0.1 WETH = 100000000000000000 Wei
3. Click "Get Quote"
4. View the quote results including:
   - Amount of USDC you would receive
   - WETH/USDC exchange price
   - Number of ticks crossed
   - Gas estimates

## How It Works

### Method 1: eth_call (Simple RPC)
1. Builds the calldata for the Uniswap quoter contract
2. Makes a direct `eth_call` RPC request to the node
3. Decodes and displays the hex result
4. **Fast and lightweight** - just one RPC call

### Method 2: Solenoid Executor (Full EVM Simulation)
The Solenoid EVM executor is compiled to WebAssembly and runs directly in your browser:

1. Fetches the latest block header from the RPC
2. Creates an `Ext` (external state) that can fetch contract code and storage on-demand
3. Executes the entire Uniswap quoter contract locally using the Solenoid EVM
4. Simulates all opcodes, state reads, and contract calls
5. Returns detailed execution results including accurate gas calculations

**The difference:** The Solenoid method actually runs the EVM bytecode in your browser, giving you complete visibility into the execution and accurate gas metering. This is the same executor used in the native examples, now running in WASM!

All computation happens in your browser - no backend required!
