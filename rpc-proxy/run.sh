#!/bin/sh
cargo build --release --quiet
CARGO_LOG=warn ./target/release/rpc-proxy
