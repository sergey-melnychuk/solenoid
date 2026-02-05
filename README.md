solenoid
========

Lightweight, async-first, WASM-friendly Ethereum VM implementation in Rust.

### Why Solenoid?

Existing EVMs (geth, reth) are synchronous and not WASM-friendly. Solenoid provides:
- **Async-first design**: Non-blocking I/O for better throughput
- **WASM-native**: Runs in browser, edge computing, embedded systems
- **Lightweight**: Embeddable as library, minimal dependencies

### RPC proxy setup

```
$ cp .env.example .env
$ cd rpc-proxy
$ cp .env.example .env
$ cargo build --release
$ ./target/release/rpc-proxy
# keep it running in background
```

### Mainnet block 23624962

```
$ cargo run --release --example runner -- 23624962
...
📦 Fetched block number: 23624962 [with 129 txs]
📦 Fetched block number: 23624962 [with 129 txs]

(total: 129, matched: 129, invalid: 0)
```

### UniswapV3 QuoterV2

```
$ etc/quoter.sh
...
📊 QuoterV2 Results:
  💰 Amount Out: 1 WETH for 3943.532812 USDC
  📊 Price After (WETH/USDC): 3955.222269012662
  🎯 Initialized Ticks Crossed: 1
  ⛽ Gas Estimate: 84919
✅ Transaction executed successfully!
🔄 Reverted: false
⛽ Gas used: 123290
...
📊 QuoterV2 Results:
  💰 Amount Out: 1 WETH for 3943.532812 USDC
  📊 Price After (WETH/USDC): 3955.222269012662
  🎯 Initialized Ticks Crossed: 1
  ⛽ Gas Estimate: 84919
✅ Transaction executed successfully!
🔄 Reverted: false
⛽ Gas used: 123290
```

### WASM support out-of-the-box

```
cd wasm-demo/
cargo check --target wasm32-unknown-unknown
wasm-pack build --target web
```
