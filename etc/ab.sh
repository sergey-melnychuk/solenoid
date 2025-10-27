#!/bin/sh

BLOCK=$1
SKIP=$2

cargo run --release --example revm -- $BLOCK $SKIP
cargo run --release --example sole -- $BLOCK $SKIP
# RUST_BACKTRACE=1 cargo run --example sole -- $BLOCK $SKIP
cargo run --release --example check -- $BLOCK $SKIP '{}' --compact
# cargo run --release --example check -- $BLOCK $SKIP '{}' --compact
