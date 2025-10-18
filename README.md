solenoid
========

WIP opinionated, lightweight, async-ready, WASM-friendly EVM in Rust.

### Uniswap V3 QuoterV2:

#### solenoid

```
$ time ./target/release/examples/quoter-sole
📦 Using block number: 23448157
c6a5026a
000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2
000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48
0000000000000000000000000000000000000000000000000de0b6b3a7640000
0000000000000000000000000000000000000000000000000000000000000bb8
0000000000000000000000000000000000000000000000000000000000000000
---
RET:
00000000000000000000000000000000000000000000000000000000eb0d890c
0000000000000000000000000000000000003e1ca359ec51e2940a31864fe74f
0000000000000000000000000000000000000000000000000000000000000001
0000000000000000000000000000000000000000000000000000000000014bb7
📊 QuoterV2 Results:
  💰 Amount Out: 1 WETH for 3943.532812 USDC
  📊 Price After (WETH/USDC): 3955.222269012662
  🎯 Initialized Ticks Crossed: 1
  ⛽ Gas Estimate: 84919
DEBUG: gas.used=101010 gas.refund=0 refund.cap=0 gas.final=123290
DEBUG: call_cost=21000, data_cost=1280
✅ Transaction executed successfully!
🔄 Reverted: false
⛽ Gas used: 123290
TRACES: 9019 in quoter-sole.log
./target/release/examples/quoter-sole  0,08s user 0,02s system 94% cpu 0,113 total
```

#### revm

```
$ time ./target/release/examples/quoter-revm
📦 Using block number: 23448157
c6a5026a
000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2
000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48
0000000000000000000000000000000000000000000000000de0b6b3a7640000
0000000000000000000000000000000000000000000000000000000000000bb8
0000000000000000000000000000000000000000000000000000000000000000
---
DEBUG revm: gas.spent=123290 gas.refund=0 refund.cap=0 gas.final=123290
RET:
00000000000000000000000000000000000000000000000000000000eb0d890c
0000000000000000000000000000000000003e1ca359ec51e2940a31864fe74f
0000000000000000000000000000000000000000000000000000000000000001
0000000000000000000000000000000000000000000000000000000000014bb7
📊 QuoterV2 Results:
  💰 Amount Out: 1 WETH for 3943.532812 USDC
  📊 Price After (WETH/USDC): 3955.222269012662
  🎯 Initialized Ticks Crossed: 1
  ⛽ Gas Estimate: 84919
✅ Transaction executed successfully!
🔄 Reverted: false
⛽ Gas used: 123290
TRACES: 9019 in quoter-revm.log
./target/release/examples/quoter-revm  0,08s user 0,02s system 22% cpu 0,465 total
```

#### check (per-opcode traces are identical)

```
$ time ./target/release/examples/quoter-check quoter-revm.log quoter-sole.log
NOTE: len match: 9019
OK
./target/release/examples/quoter-check quoter-revm.log quoter-sole.log  0,08s user 0,02s system 20% cpu 0,479 total
```

---

```
cargo build --target wasm32-unknown-unknown
```
