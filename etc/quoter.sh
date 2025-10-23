#!/bin/sh
echo "Run Uniswap V3 Quoter V2: Gas Parity with REVM"
time RUST_LOG=off cargo run --release --example quoter-revm
time RUST_LOG=off cargo run --release --example quoter-sole
cargo run --release --example quoter-check -- quoter-revm.log quoter-sole.log
