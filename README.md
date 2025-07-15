solenoid
========

WIP opinionated, lightweight, async-ready, WASM-friendly EVM impl in Rust.

```
## Counter.get()
cargo run --release -- 0x$(cat etc/counter/Counter.bin-runtime) 0x6d4ce63c
...
OK: 0x0000000000000000000000000000000000000000000000000000000000000000
R:0xe7f1725e7734ce288f8367e1bb143e90bb3f0512[0]=0

## Counter.set(uint256)
cargo run --release -- 0x$(cat etc/counter/Counter.bin-runtime) 0x60fe47b10000000000000000000000000000000000000000000000000000000000000042
...
OK: 0x
W:0xe7f1725e7734ce288f8367e1bb143e90bb3f0512[0]=0->42

## Counter.inc()
cargo run --release -- 0x$(cat etc/counter/Counter.bin-runtime) 0x371303c0
...
OK: 0x
R:0xe7f1725e7734ce288f8367e1bb143e90bb3f0512[0]=0
W:0xe7f1725e7734ce288f8367e1bb143e90bb3f0512[0]=0->1

## Counter.dec()
cargo run --release -- 0x$(cat etc/counter/Counter.bin-runtime) 0xb3bcfa82
...
REVERTED: 0x4e487b710000000000000000000000000000000000000000000000000000000000000011
R:0xe7f1725e7734ce288f8367e1bb143e90bb3f0512[0]=0
```

```
cargo check --target wasm32-unknown-unknown
```
