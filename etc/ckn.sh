#!/bin/sh

BLOCK=$1
SKIP=$2

## Non-interactive version of `check`: just show opcode that mismatched
cargo run --release --example check -- $BLOCK $SKIP --compact --noninteractive 2> /dev/null
